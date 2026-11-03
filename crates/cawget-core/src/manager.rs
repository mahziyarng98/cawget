//! The job manager: queue, scheduler, concurrency limit, pause/resume/remove, live progress.
//!
//! One `Manager` is shared by the CLI, the GUI and tests. `run` is the scheduler loop; the other
//! methods may be called from any task while it runs.

use crate::error::{Error, Result};
use crate::http;
use crate::job::{self, JobEnv, Outcome, Progress};
use crate::limiter::Limiter;
use crate::settings::Settings;
use crate::store::{Job, NewJob, State, Store};
use crate::util::{in_window, local_minute_of_day, unix_now};
use serde::Serialize;
use std::collections::HashMap;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{Notify, broadcast};
use tokio::time::{Duration, Instant, sleep};
use tokio_util::sync::CancellationToken;

/// Notifications for UIs and logs.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Info { id: i64, message: String },
    Done { id: i64, path: String },
    Failed { id: i64, error: String },
}

/// A job plus its live numbers, ready to display.
#[derive(Debug, Clone, Serialize)]
pub struct JobView {
    #[serde(flatten)]
    pub job: Job,
    pub done: u64,
    /// Bytes per second.
    pub speed: u64,
    pub connections: u32,
    /// 0.0..=1.0, `None` while the size is unknown.
    pub fraction: Option<f64>,
    pub eta_secs: Option<u64>,
    /// File name (last component of `out_path`, or of the URL before the first connection).
    pub name: String,
}

/// What to add to the queue.
#[derive(Debug, Clone, Default)]
pub struct AddRequest {
    pub url: String,
    /// Exact output file (takes precedence over `dir`).
    pub output: Option<String>,
    pub dir: Option<String>,
    pub segments: Option<u32>,
    pub headers: Vec<String>,
    pub referer: Option<String>,
    pub cookie: Option<String>,
    pub user_agent: Option<String>,
    pub sha256: Option<String>,
    /// Unix time to start at (0 = now).
    pub not_before: i64,
    pub paused: bool,
}

/// What a probe of an address found out (for the "file details" step before adding a download).
#[derive(Debug, Clone, Serialize)]
pub struct ProbeInfo {
    /// Bytes, if the server said.
    pub size: Option<u64>,
    /// The server honours range requests (parallel segments and resume possible).
    pub resumable: bool,
    /// File name the download would get (server-suggested or from the address).
    pub file_name: String,
    /// Address after redirects.
    pub final_url: String,
}

/// Options for [`Manager::run`].
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// Only run this job.
    pub only_id: Option<i64>,
    /// Keep waiting for new jobs instead of returning when the queue is empty.
    pub daemon: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunSummary {
    pub done: usize,
    pub failed: usize,
    /// Stopped by the shutdown token with work left.
    pub interrupted: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Reason {
    None = 0,
    Pause = 1,
    Remove = 2,
    Shutdown = 3,
}

struct Running {
    cancel: CancellationToken,
    prog: Arc<Progress>,
    reason: Arc<AtomicU8>,
}

struct Inner {
    store: Arc<Store>,
    /// Lock file taken by `run`, so only one scheduler works on a database at a time.
    lock_path: Option<std::path::PathBuf>,
    running: Mutex<HashMap<i64, Running>>,
    limiter: Arc<Limiter>,
    client: Mutex<Option<(String, reqwest::Client)>>,
    events: broadcast::Sender<Event>,
    wake: Notify,
    done: AtomicUsize,
    failed: AtomicUsize,
}

/// Cheap to clone; all clones share one queue.
#[derive(Clone)]
pub struct Manager {
    inner: Arc<Inner>,
}

impl Manager {
    pub fn open(db: &Path) -> Result<Manager> {
        let mut m = Self::with_store(Store::open(db)?);
        let mut lock = db.as_os_str().to_owned();
        lock.push(".lock");
        Arc::get_mut(&mut m.inner).expect("fresh manager").lock_path = Some(lock.into());
        Ok(m)
    }

    pub fn with_store(store: Store) -> Manager {
        let settings = Settings::load(&store);
        let (events, _) = broadcast::channel(256);
        Manager {
            inner: Arc::new(Inner {
                store: Arc::new(store),
                lock_path: None,
                running: Mutex::new(HashMap::new()),
                limiter: Arc::new(Limiter::new(settings.max_speed)),
                client: Mutex::new(None),
                events,
                wake: Notify::new(),
                done: AtomicUsize::new(0),
                failed: AtomicUsize::new(0),
            }),
        }
    }

    pub fn store(&self) -> &Store {
        &self.inner.store
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.inner.events.subscribe()
    }

    pub fn settings(&self) -> Settings {
        Settings::load(&self.inner.store)
    }

    /// Validates and stores a setting; speed limits take effect immediately.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        Settings::set(&self.inner.store, key, value)?;
        self.inner.limiter.set_rate(self.settings().max_speed);
        self.inner.wake.notify_one();
        Ok(())
    }

    // ---- queue operations ----

    fn validate(req: &AddRequest) -> Result<()> {
        let lower = req.url.to_ascii_lowercase();
        if !(lower.starts_with("http://") || lower.starts_with("https://")) {
            return Err(Error::invalid(format!(
                "only http:// and https:// URLs are supported: {}",
                req.url
            )));
        }
        if let Some(h) = req.headers.iter().find(|h| http::parse_header(h).is_none()) {
            return Err(Error::invalid(format!(
                "bad header '{h}' (expected 'Name: value')"
            )));
        }
        Ok(())
    }

    /// Asks the server about an address without adding a job: size, range support and the file name.
    /// Uses the same client, headers and settings as a real download.
    pub async fn probe_url(&self, req: &AddRequest) -> std::result::Result<ProbeInfo, String> {
        Self::validate(req).map_err(|e| e.to_string())?;
        let s = self.settings();
        let client = self.client_for(&s)?;
        let job = Job {
            id: 0,
            url: req.url.clone(),
            out_path: String::new(),
            state: State::Queued,
            referer: req.referer.clone(),
            user_agent: req.user_agent.clone(),
            headers: req.headers.clone(),
            cookie: req.cookie.clone(),
            sha256: None,
            etag: None,
            last_modified: None,
            error: None,
            size: None,
            accept_ranges: false,
            segments: 0,
            created: 0,
            not_before: 0,
        };
        let p = http::probe(&client, &job, &s.user_agent)
            .await
            .map_err(|e| e.message().to_string())?;
        let file_name = p
            .content_disposition
            .as_deref()
            .and_then(crate::util::filename_from_disposition)
            .unwrap_or_else(|| crate::util::filename_from_url(&p.final_url));
        Ok(ProbeInfo {
            size: p.size,
            resumable: p.ranges,
            file_name,
            final_url: p.final_url,
        })
    }

    pub fn add(&self, req: AddRequest) -> Result<i64> {
        Self::validate(&req)?;
        if let Some(s) = &req.sha256
            && (s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(Error::invalid("sha256 must be 64 hex digits"));
        }
        if req.segments.is_some_and(|n| !(1..=64).contains(&n)) {
            return Err(Error::invalid("segments must be 1..64"));
        }
        let abs = |p: &str| -> Result<String> {
            let path = Path::new(p);
            let full = if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()?.join(path)
            };
            Ok(full.to_string_lossy().into_owned())
        };
        let out_path = if let Some(o) = &req.output {
            let full = abs(o)?;
            if let Some(dir) = Path::new(&full).parent() {
                std::fs::create_dir_all(dir)?;
            }
            full
        } else if let Some(d) = &req.dir {
            let dir = std::path::PathBuf::from(abs(d)?);
            std::fs::create_dir_all(&dir)?;
            format!("{}/", dir.display().to_string().trim_end_matches('/'))
        } else {
            // No folder chosen: decided on first contact, when the file name (and so its category) is known.
            String::new()
        };
        let id = self.inner.store.add_job(
            &NewJob {
                url: req.url,
                out_path,
                paused: req.paused,
                referer: req.referer,
                user_agent: req.user_agent,
                headers: req.headers,
                cookie: req.cookie,
                sha256: req.sha256.map(|s| s.to_ascii_lowercase()),
                segments: req.segments.unwrap_or(0),
                not_before: req.not_before,
            },
            unix_now(),
        )?;
        self.inner.wake.notify_one();
        Ok(id)
    }

    /// Stops a queued or running job, keeping its progress.
    pub fn pause(&self, id: i64) -> Result<()> {
        let job = self.inner.store.require_job(id)?;
        if !matches!(job.state, State::Queued | State::Active) {
            return Err(Error::WrongState {
                id,
                state: job.state.as_str().into(),
            });
        }
        self.inner.store.set_state(id, State::Paused, None)?;
        self.cancel_running(id, Reason::Pause);
        Ok(())
    }

    /// Queues a paused or failed job again (progress is kept where possible).
    pub fn resume(&self, id: i64) -> Result<()> {
        let job = self.inner.store.require_job(id)?;
        if !matches!(job.state, State::Paused | State::Failed) {
            return Err(Error::WrongState {
                id,
                state: job.state.as_str().into(),
            });
        }
        self.inner.store.set_state(id, State::Queued, None)?;
        self.inner.wake.notify_one();
        Ok(())
    }

    /// Forgets a job. Partial data is deleted; a finished file only with `delete_file`.
    pub fn remove(&self, id: i64, delete_file: bool) -> Result<()> {
        let job = self.inner.store.require_job(id)?;
        self.cancel_running(id, Reason::Remove);
        self.inner.store.delete_job(id)?;
        if !job.out_path.is_empty() && !job.out_path.ends_with('/') {
            if job.state != State::Done {
                let _ = std::fs::remove_file(format!("{}.part", job.out_path));
            } else if delete_file {
                let _ = std::fs::remove_file(&job.out_path);
            }
        }
        Ok(())
    }

    /// Removes all finished jobs from the list (files stay). Returns how many.
    pub fn clear_finished(&self) -> Result<usize> {
        let done = self.inner.store.list_jobs(Some(State::Done))?;
        for j in &done {
            self.inner.store.delete_job(j.id)?;
        }
        Ok(done.len())
    }

    pub fn set_schedule(&self, id: i64, not_before: i64) -> Result<()> {
        self.inner.store.require_job(id)?;
        self.inner.store.set_schedule(id, not_before)?;
        self.inner.wake.notify_one();
        Ok(())
    }

    fn cancel_running(&self, id: i64, reason: Reason) {
        let map = self.inner.running.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(r) = map.get(&id) {
            r.reason.store(reason as u8, Ordering::SeqCst);
            r.cancel.cancel();
        }
    }

    // ---- views ----

    fn view(
        &self,
        job: Job,
        saved: &HashMap<i64, u64>,
        live: &HashMap<i64, (u64, u64, u32, Option<u64>)>,
    ) -> JobView {
        let (done, speed, connections, size) = match live.get(&job.id) {
            Some(&(d, s, c, sz)) => (d, s, c, sz.or(job.size)),
            None => (
                if job.state == State::Done {
                    job.size.unwrap_or(0)
                } else {
                    saved.get(&job.id).copied().unwrap_or(0)
                },
                0,
                0,
                job.size,
            ),
        };
        let fraction = size
            .filter(|&s| s > 0)
            .map(|s| (done as f64 / s as f64).min(1.0))
            .or(if job.state == State::Done {
                Some(1.0)
            } else {
                None
            });
        let eta_secs = match size {
            Some(s) if speed > 0 && s > done => Some((s - done) / speed),
            _ => None,
        };
        let name = if job.out_path.is_empty() || job.out_path.ends_with('/') {
            crate::util::filename_from_url(&job.url)
        } else {
            job.out_path.rsplit('/').next().unwrap_or("").to_string()
        };
        JobView {
            done,
            speed,
            connections,
            fraction,
            eta_secs,
            name,
            job,
        }
    }

    fn live_map(&self) -> HashMap<i64, (u64, u64, u32, Option<u64>)> {
        let map = self.inner.running.lock().unwrap_or_else(|e| e.into_inner());
        map.iter()
            .map(|(&id, r)| {
                (
                    id,
                    (
                        r.prog.done.load(Ordering::Relaxed),
                        r.prog.speed.load(Ordering::Relaxed),
                        r.prog.connections.load(Ordering::Relaxed),
                        r.prog.size(),
                    ),
                )
            })
            .collect()
    }

    /// All jobs, newest last, with live progress for running ones.
    pub fn list(&self) -> Result<Vec<JobView>> {
        let jobs = self.inner.store.list_jobs(None)?;
        let saved = self.inner.store.done_bytes_all()?;
        let live = self.live_map();
        Ok(jobs
            .into_iter()
            .map(|j| self.view(j, &saved, &live))
            .collect())
    }

    pub fn get(&self, id: i64) -> Result<JobView> {
        let job = self.inner.store.require_job(id)?;
        let mut saved = HashMap::new();
        saved.insert(id, self.inner.store.done_bytes(id)?);
        Ok(self.view(job, &saved, &self.live_map()))
    }

    // ---- scheduler ----

    fn client_for(&self, s: &Settings) -> std::result::Result<reqwest::Client, String> {
        let key = format!("{:?}|{}|{}", s.cacert, s.connect_timeout, s.proxy);
        let mut slot = self.inner.client.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((k, c)) = slot.as_ref()
            && *k == key
        {
            return Ok(c.clone());
        }
        let c = http::build_client(s)?;
        *slot = Some((key, c.clone()));
        Ok(c)
    }

    fn next_runnable(&self, now: i64, s: &Settings, only: Option<i64>) -> Option<Job> {
        if let Some((a, b)) = s.window
            && !in_window(local_minute_of_day(now), a, b)
        {
            return None;
        }
        let running = self.inner.running.lock().unwrap_or_else(|e| e.into_inner());
        self.inner
            .store
            .list_jobs(Some(State::Queued))
            .ok()?
            .into_iter()
            .find(|j| {
                j.not_before <= now
                    && only.is_none_or(|o| o == j.id)
                    && !running.contains_key(&j.id)
            })
    }

    /// Seconds until the next queued job could start, if any is waiting for a time or window.
    fn next_wakeup(&self, now: i64, s: &Settings, only: Option<i64>) -> Option<i64> {
        let queued = self.inner.store.list_jobs(Some(State::Queued)).ok()?;
        queued
            .iter()
            .filter(|j| only.is_none_or(|o| o == j.id))
            .filter_map(|j| {
                let mut t = j.not_before.max(now);
                if let Some((a, b)) = s.window {
                    let mut guard = 0;
                    while !in_window(local_minute_of_day(t), a, b) && guard < 24 * 60 {
                        t += 60 - t.rem_euclid(60);
                        guard += 1;
                    }
                }
                (t > now).then_some(t)
            })
            .min()
    }

    fn spawn_job(&self, tasks: &mut tokio::task::JoinSet<()>, job: Job, s: Settings) {
        let id = job.id;
        let client = match self.client_for(&s) {
            Ok(c) => c,
            Err(m) => {
                let _ = self.inner.store.set_state(id, State::Failed, Some(&m));
                self.inner.failed.fetch_add(1, Ordering::Relaxed);
                let _ = self.inner.events.send(Event::Failed { id, error: m });
                return;
            }
        };
        let _ = self.inner.store.set_state(id, State::Active, None);
        let prog = Progress::new();
        let cancel = CancellationToken::new();
        let reason = Arc::new(AtomicU8::new(Reason::None as u8));
        self.inner
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                id,
                Running {
                    cancel: cancel.clone(),
                    prog: Arc::clone(&prog),
                    reason: Arc::clone(&reason),
                },
            );
        let mut job = job;
        if job.segments == 0 {
            job.segments = s.segments;
        }
        let env = JobEnv {
            store: Arc::clone(&self.inner.store),
            client,
            settings: s,
            limiter: Arc::clone(&self.inner.limiter),
            events: self.inner.events.clone(),
        };
        let me = self.clone();
        tasks.spawn(async move {
            let outcome = job::run_job(env, job, cancel, prog).await;
            let inner = &me.inner;
            match outcome {
                Outcome::Done(path) => {
                    let _ = inner.store.set_state(id, State::Done, None);
                    inner.done.fetch_add(1, Ordering::Relaxed);
                    let _ = inner.events.send(Event::Done {
                        id,
                        path: path.to_string_lossy().into_owned(),
                    });
                }
                Outcome::Failed(error) => {
                    if inner.store.job_state(id).ok().flatten().is_some() {
                        let _ = inner.store.set_state(id, State::Failed, Some(&error));
                    }
                    inner.failed.fetch_add(1, Ordering::Relaxed);
                    let _ = inner.events.send(Event::Failed { id, error });
                }
                Outcome::Cancelled => {
                    let why = inner
                        .running
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .get(&id)
                        .map(|r| r.reason.load(Ordering::SeqCst));
                    // Pause and remove already updated the database; a shutdown (or an external
                    // change noticed by polling) must leave the job resumable.
                    if why == Some(Reason::Shutdown as u8)
                        && inner.store.job_state(id).ok().flatten() == Some(State::Active)
                    {
                        let _ = inner.store.set_state(id, State::Queued, None);
                    }
                }
            }
            inner
                .running
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
            inner.wake.notify_one();
        });
    }

    /// The scheduler loop. Starts queued jobs as slots, time and the daily window allow, applies
    /// pause/remove requests (also from other processes sharing the database), and returns when
    /// the queue is drained (unless `daemon`) or `shutdown` is cancelled (progress is saved and
    /// running jobs are re-queued).
    ///
    /// Fails with [`Error::Busy`] if another process is already running the scheduler on the same
    /// database file.
    pub async fn run(&self, opts: RunOptions, shutdown: CancellationToken) -> Result<RunSummary> {
        let inner = &self.inner;
        let _engine_lock = match &inner.lock_path {
            Some(p) => {
                let f = std::fs::OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .write(true)
                    .mode(0o600)
                    .open(p)?;
                match rustix::fs::flock(&f, rustix::fs::FlockOperation::NonBlockingLockExclusive) {
                    Ok(()) => Some(f), // released when `f` drops, also if the process dies
                    Err(e) if e == rustix::io::Errno::WOULDBLOCK => return Err(Error::Busy),
                    Err(e) => return Err(Error::Io(e.into())),
                }
            }
            None => None,
        };
        let _ = inner.store.reset_active(); // leftovers of a crashed process
        inner.done.store(0, Ordering::Relaxed);
        inner.failed.store(0, Ordering::Relaxed);
        let mut tasks = tokio::task::JoinSet::new();
        let mut last_poll = Instant::now();
        let mut announced_wait = 0i64;
        let mut interrupted = false;

        loop {
            while tasks.try_join_next().is_some() {}
            if shutdown.is_cancelled() {
                interrupted = true;
                break;
            }
            let settings = self.settings();
            inner.limiter.set_rate(settings.max_speed);
            let now = unix_now();

            if last_poll.elapsed() >= Duration::from_secs(1) {
                last_poll = Instant::now();
                let ids: Vec<i64> = inner
                    .running
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .keys()
                    .copied()
                    .collect();
                for id in ids {
                    match inner.store.job_state(id) {
                        Ok(None) => self.cancel_running(id, Reason::Remove),
                        Ok(Some(State::Paused)) => self.cancel_running(id, Reason::Pause),
                        _ => {}
                    }
                }
            }

            loop {
                let n = inner
                    .running
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .len();
                if n >= settings.max_jobs as usize {
                    break;
                }
                let Some(job) = self.next_runnable(now, &settings, opts.only_id) else {
                    break;
                };
                self.spawn_job(&mut tasks, job, settings.clone());
            }

            if !opts.daemon
                && inner
                    .running
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .is_empty()
            {
                match self.next_wakeup(now, &settings, opts.only_id) {
                    None => {
                        let waiting = self.next_runnable(now, &settings, opts.only_id).is_some();
                        if !waiting {
                            break;
                        }
                    }
                    Some(when) => {
                        if announced_wait != when {
                            announced_wait = when;
                            let _ = inner.events.send(Event::Info {
                                id: 0,
                                message: format!(
                                    "waiting until {} for scheduled jobs",
                                    fmt_local(when)
                                ),
                            });
                        }
                    }
                }
            }

            tokio::select! {
                _ = shutdown.cancelled() => {}
                _ = inner.wake.notified() => {}
                _ = sleep(Duration::from_millis(200)) => {}
            }
        }

        if interrupted {
            let map = inner.running.lock().unwrap_or_else(|e| e.into_inner());
            for r in map.values() {
                r.reason.store(Reason::Shutdown as u8, Ordering::SeqCst);
                r.cancel.cancel();
            }
        }
        while tasks.join_next().await.is_some() {}
        Ok(RunSummary {
            done: inner.done.load(Ordering::Relaxed),
            failed: inner.failed.load(Ordering::Relaxed),
            interrupted,
        })
    }
}

fn fmt_local(ts: i64) -> String {
    use chrono::TimeZone;
    chrono::Local.timestamp_opt(ts, 0).single().map_or_else(
        || ts.to_string(),
        |d| d.format("%Y-%m-%d %H:%M:%S").to_string(),
    )
}
