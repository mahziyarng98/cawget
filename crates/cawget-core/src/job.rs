//! The download of one job: probe, plan or resume, parallel range workers with work stealing,
//! periodic checkpoints, and finalisation (rename, checksum, modification time).

use crate::http::{self, FetchError, Probe};
use crate::limiter::Limiter;
use crate::manager::Event;
use crate::segment::{self, Segment};
use crate::settings::Settings;
use crate::store::{Job, Store};
use crate::util::{filename_from_disposition, filename_from_url, fmt_bytes, unique_path};
use futures_util::StreamExt;
use reqwest::{Client, header};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::os::unix::fs::FileExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tokio::time::{Duration, Instant, sleep};
use tokio_util::sync::CancellationToken;

/// Live numbers shared with whoever displays the job.
#[derive(Debug)]
pub struct Progress {
    pub done: AtomicU64,
    /// `u64::MAX` while the size is unknown.
    pub size: AtomicU64,
    /// Smoothed bytes per second.
    pub speed: AtomicU64,
    pub connections: AtomicU32,
    /// 0 connecting, 1 downloading, 2 finishing.
    pub phase: AtomicU32,
}

impl Progress {
    pub fn new() -> Arc<Progress> {
        Arc::new(Progress {
            done: AtomicU64::new(0),
            size: AtomicU64::new(u64::MAX),
            speed: AtomicU64::new(0),
            connections: AtomicU32::new(0),
            phase: AtomicU32::new(0),
        })
    }
    pub fn size(&self) -> Option<u64> {
        let s = self.size.load(Ordering::Relaxed);
        (s != u64::MAX).then_some(s)
    }
}

/// How a job run ended.
#[derive(Debug)]
pub enum Outcome {
    Done(PathBuf),
    Failed(String),
    /// Stopped from outside (pause, remove, shutdown). Progress is saved.
    Cancelled,
}

/// Everything a job run needs from its surroundings.
#[derive(Clone)]
pub struct JobEnv {
    pub store: Arc<Store>,
    pub client: Client,
    pub settings: Settings,
    pub limiter: Arc<Limiter>,
    pub events: broadcast::Sender<Event>,
}

impl JobEnv {
    fn info(&self, id: i64, message: String) {
        let _ = self.events.send(Event::Info { id, message });
    }
}

fn backoff(attempt: u32) -> Duration {
    Duration::from_millis((400u64 << attempt.min(6)).min(20_000))
}

#[derive(Debug)]
enum Stop {
    Cancelled,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Run {
    Idle,
    Running,
    Waiting(Instant),
    Done,
}

#[derive(Debug)]
struct Slot {
    seg: Segment,
    run: Run,
    retries: u32,
}

enum Claim {
    Work { idx: usize, offset: u64, end: u64 },
    Wait(Duration),
    AllDone,
}

/// State shared by the workers of one job.
struct Shared {
    env: JobEnv,
    job: Job,
    url: String,
    file: Arc<File>,
    slots: Mutex<Vec<Slot>>,
    ranges: bool,
    want: usize,
    prog: Arc<Progress>,
    cancel: CancellationToken,
    fatal: Mutex<Option<String>>,
}

struct ConnGuard<'a>(&'a AtomicU32);
impl<'a> ConnGuard<'a> {
    fn new(c: &'a AtomicU32) -> Self {
        c.fetch_add(1, Ordering::Relaxed);
        ConnGuard(c)
    }
}
impl Drop for ConnGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Slot>> {
        self.slots.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn set_fatal(&self, msg: String) {
        let mut f = self.fatal.lock().unwrap_or_else(|e| e.into_inner());
        f.get_or_insert(msg);
        self.cancel.cancel();
    }

    fn segments(&self) -> Vec<Segment> {
        self.lock().iter().map(|s| s.seg).collect()
    }

    /// Next piece of work for an idle worker: a ready segment, or the tail of a running one.
    fn claim(&self) -> Claim {
        let mut g = self.lock();
        let now = Instant::now();
        for s in g.iter_mut() {
            if s.seg.is_complete() {
                s.run = Run::Done;
            }
        }
        if g.iter().all(|s| s.run == Run::Done) {
            return Claim::AllDone;
        }
        let ready = g.iter().position(|s| match s.run {
            Run::Idle => true,
            Run::Waiting(t) => t <= now,
            _ => false,
        });
        if let Some(i) = ready {
            g[i].run = Run::Running;
            return Claim::Work {
                idx: i,
                offset: g[i].seg.offset(),
                end: g[i].seg.end,
            };
        }
        if self.ranges {
            let mut copy: Vec<Segment> = g.iter().map(|s| s.seg).collect();
            let min_piece = self.env.settings.min_segment_size;
            if let Some((victim, created)) =
                segment::steal(&mut copy, min_piece, |i| g[i].run == Run::Running)
            {
                g[victim].seg.end = copy[victim].end;
                let tail = copy[created];
                g.push(Slot {
                    seg: tail,
                    run: Run::Running,
                    retries: 0,
                });
                return Claim::Work {
                    idx: created,
                    offset: tail.offset(),
                    end: tail.end,
                };
            }
        }
        let wake = g
            .iter()
            .filter_map(|s| {
                if let Run::Waiting(t) = s.run {
                    Some(t.saturating_duration_since(now))
                } else {
                    None
                }
            })
            .min()
            .unwrap_or(Duration::from_millis(150))
            .min(Duration::from_millis(150));
        Claim::Wait(wake)
    }

    async fn write_at(&self, data: bytes::Bytes, offset: u64) -> std::io::Result<()> {
        let f = Arc::clone(&self.file);
        tokio::task::spawn_blocking(move || f.write_all_at(&data, offset))
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?
    }

    /// Downloads `offset..=end` of segment `idx`. Returns when the segment is complete.
    async fn fetch(&self, idx: usize, offset: u64, end: u64) -> Result<(), FetchOrStop> {
        let mut rb = http::request(
            &self.env.client,
            &self.job,
            &self.env.settings.user_agent,
            &self.url,
        );
        if self.ranges {
            rb = rb.header(header::RANGE, format!("bytes={offset}-{end}"));
        }
        let _conn = ConnGuard::new(&self.prog.connections);
        let resp = tokio::select! {
            _ = self.cancel.cancelled() => return Err(FetchOrStop::Cancelled),
            r = rb.send() => r.map_err(|e| FetchOrStop::Fetch(http::classify_error(&e)))?,
        };
        let status = resp.status();
        if let Some(e) = http::classify_status(status) {
            return Err(FetchOrStop::Fetch(e));
        }
        if self.ranges {
            if status.as_u16() != 206 {
                return Err(FetchOrStop::Fetch(FetchError::Transient(format!(
                    "server answered HTTP {} to a range request",
                    status.as_u16()
                ))));
            }
            let start = resp
                .headers()
                .get(header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(http::content_range_start);
            if start != Some(offset) {
                return Err(FetchOrStop::Fetch(FetchError::Transient(
                    "server returned a different range".into(),
                )));
            }
        } else if status.as_u16() != 200 {
            return Err(FetchOrStop::Fetch(FetchError::Transient(format!(
                "unexpected HTTP {}",
                status.as_u16()
            ))));
        }

        let mut stream = resp.bytes_stream();
        loop {
            let next = tokio::select! {
                _ = self.cancel.cancelled() => return Err(FetchOrStop::Cancelled),
                n = stream.next() => n,
            };
            let Some(chunk) = next else { break };
            let chunk = chunk.map_err(|e| FetchOrStop::Fetch(http::classify_error(&e)))?;
            tokio::select! {
                _ = self.cancel.cancelled() => return Err(FetchOrStop::Cancelled),
                _ = self.env.limiter.acquire(chunk.len()) => {}
            }
            // The segment may have been shortened by work stealing since the request was made.
            let (at, take) = {
                let g = self.lock();
                let s = &g[idx].seg;
                (
                    s.offset(),
                    (s.end + 1)
                        .saturating_sub(s.offset())
                        .min(chunk.len() as u64) as usize,
                )
            };
            if take > 0 {
                self.write_at(chunk.slice(..take), at)
                    .await
                    .map_err(|e| FetchOrStop::Io(e.to_string()))?;
                self.lock()[idx].seg.done += take as u64;
                self.prog.done.fetch_add(take as u64, Ordering::Relaxed);
            }
            if take < chunk.len() || self.lock()[idx].seg.is_complete() {
                return Ok(()); // finished, or crossed the new end: stop the longer request
            }
        }
        if self.lock()[idx].seg.is_complete() {
            Ok(())
        } else {
            Err(FetchOrStop::Fetch(FetchError::Transient(
                "transfer ended early".into(),
            )))
        }
    }

    async fn worker(self: Arc<Self>) {
        loop {
            if self.cancel.is_cancelled() {
                return;
            }
            match self.claim() {
                Claim::AllDone => return,
                Claim::Wait(d) => {
                    tokio::select! { _ = self.cancel.cancelled() => return, _ = sleep(d) => {} }
                }
                Claim::Work { idx, offset, end } => match self.fetch(idx, offset, end).await {
                    Ok(()) => self.lock()[idx].run = Run::Done,
                    Err(FetchOrStop::Cancelled) => return,
                    Err(FetchOrStop::Io(m)) => return self.set_fatal(format!("write error: {m}")),
                    Err(FetchOrStop::Fetch(FetchError::Permanent(m))) => return self.set_fatal(m),
                    Err(FetchOrStop::Fetch(FetchError::Transient(m))) => {
                        self.segment_failed(idx, m)
                    }
                },
            }
        }
    }

    fn segment_failed(&self, idx: usize, why: String) {
        let max = self.env.settings.retries;
        let mut g = self.lock();
        let slot = &mut g[idx];
        slot.retries += 1;
        if slot.retries > max {
            drop(g);
            return self.set_fatal(format!("segment {idx} failed after {max} retries: {why}"));
        }
        if !self.ranges {
            self.prog.done.fetch_sub(slot.seg.done, Ordering::Relaxed);
            slot.seg.done = 0; // a plain stream cannot continue mid-way
        }
        slot.run = Run::Waiting(Instant::now() + backoff(slot.retries));
        let msg = format!("segment {idx} ({why}), attempt {}/{max}", slot.retries);
        drop(g);
        self.env.info(self.job.id, format!("retry: {msg}"));
    }

    /// Flushes data to disk, then records progress. Order matters: the database must never claim
    /// more than the disk holds.
    async fn checkpoint(&self) {
        let f = Arc::clone(&self.file);
        let _ = tokio::task::spawn_blocking(move || f.sync_data()).await;
        let _ = self.env.store.save_segments(self.job.id, &self.segments());
    }
}

enum FetchOrStop {
    Cancelled,
    Io(String),
    Fetch(FetchError),
}

async fn sleep_or_cancel(d: Duration, cancel: &CancellationToken) -> bool {
    tokio::select! { _ = cancel.cancelled() => false, _ = sleep(d) => true }
}

async fn probe_with_retries(
    env: &JobEnv,
    job: &Job,
    cancel: &CancellationToken,
) -> Result<Probe, Stop> {
    let mut attempt = 0;
    loop {
        let r = tokio::select! {
            _ = cancel.cancelled() => return Err(Stop::Cancelled),
            r = http::probe(&env.client, job, &env.settings.user_agent) => r,
        };
        match r {
            Ok(p) => return Ok(p),
            Err(FetchError::Permanent(m)) => return Err(Stop::Failed(m)),
            Err(FetchError::Transient(m)) => {
                attempt += 1;
                if attempt > env.settings.retries {
                    return Err(Stop::Failed(m));
                }
                env.info(
                    job.id,
                    format!(
                        "retry: connect ({m}), attempt {attempt}/{}",
                        env.settings.retries
                    ),
                );
                if !sleep_or_cancel(backoff(attempt), cancel).await {
                    return Err(Stop::Cancelled);
                }
            }
        }
    }
}

/// Reserves disk space; falls back to a sparse file where `fallocate` is unsupported.
fn reserve(file: &File, size: u64) -> Result<(), String> {
    match rustix::fs::fallocate(file, rustix::fs::FallocateFlags::empty(), 0, size) {
        Ok(()) => Ok(()),
        Err(e) if e == rustix::io::Errno::OPNOTSUPP || e == rustix::io::Errno::NOSYS => file
            .set_len(size)
            .map_err(|e| format!("cannot size file: {e}")),
        Err(e) if e == rustix::io::Errno::NOSPC => Err(format!(
            "not enough disk space for {}",
            fmt_bytes(size as f64)
        )),
        Err(e) => Err(format!("cannot reserve {} bytes: {e}", size)),
    }
}

/// Runs one job to completion, failure or cancellation.
pub async fn run_job(
    env: JobEnv,
    mut job: Job,
    cancel: CancellationToken,
    prog: Arc<Progress>,
) -> Outcome {
    let probe = match probe_with_retries(&env, &job, &cancel).await {
        Ok(p) => p,
        Err(Stop::Cancelled) => return Outcome::Cancelled,
        Err(Stop::Failed(m)) => return Outcome::Failed(m),
    };

    // Where the file goes.
    if job.out_path.is_empty() || job.out_path.ends_with('/') {
        let name = probe
            .content_disposition
            .as_deref()
            .and_then(filename_from_disposition)
            .unwrap_or_else(|| filename_from_url(&probe.final_url));
        let dir = if job.out_path.is_empty() {
            env.settings.folder_for(&name) // no folder chosen: category folder or default folder
        } else {
            PathBuf::from(&job.out_path)
        };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            return Outcome::Failed(format!("cannot create {}: {e}", dir.display()));
        }
        job.out_path = unique_path(&dir.join(name)).to_string_lossy().into_owned();
    }
    let part = PathBuf::from(format!("{}.part", job.out_path));

    // Can saved progress be trusted?
    let saved = env.store.load_segments(job.id).unwrap_or_default();
    let mut reuse = !saved.is_empty()
        && probe.ranges
        && probe.size.is_some()
        && job.size == probe.size
        && segment::validate(&saved, probe.size.unwrap_or(0))
        && !matches!((&job.etag, &probe.etag), (Some(a), Some(b)) if a != b)
        && !matches!((&job.last_modified, &probe.last_modified), (Some(a), Some(b)) if a != b)
        && std::fs::metadata(&part)
            .map(|m| Some(m.len()) == probe.size)
            .unwrap_or(false);
    if !saved.is_empty() && !reuse {
        env.info(
            job.id,
            "restart: saved progress cannot be reused (remote changed, no ranges or file missing)"
                .into(),
        );
    }

    job.size = probe.size;
    job.accept_ranges = probe.ranges;
    job.etag = probe.etag.clone();
    job.last_modified = probe.last_modified.clone();
    let _ = env.store.set_meta(&job);
    if let Some(s) = probe.size {
        prog.size.store(s, Ordering::Relaxed);
    }

    let file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&part)
    {
        Ok(f) => f,
        Err(e) => return Outcome::Failed(format!("cannot open {}: {e}", part.display())),
    };
    if !reuse {
        if let Err(e) = file.set_len(0) {
            return Outcome::Failed(format!("cannot reset {}: {e}", part.display()));
        }
        if let Some(sz) = probe.size.filter(|&s| s > 0)
            && let Err(m) = reserve(&file, sz)
        {
            return Outcome::Failed(m);
        }
    }
    let file = Arc::new(file);
    let name = job.out_path.rsplit('/').next().unwrap_or("").to_string();

    let Some(size) = probe.size else {
        return stream_unknown_size(env, job, probe, file, part, cancel, prog).await;
    };

    let segs = if reuse {
        env.info(
            job.id,
            format!(
                "resume: {name} at {:.1}%",
                100.0 * segment::done_total(&saved) as f64 / size as f64
            ),
        );
        saved
    } else if probe.ranges {
        segment::plan(
            size,
            job.segments.max(1) as usize,
            env.settings.min_segment_size,
        )
    } else if size > 0 {
        vec![Segment::new(0, size - 1)]
    } else {
        Vec::new()
    };
    reuse = false; // not needed any more
    let _ = reuse;
    prog.done
        .store(segment::done_total(&segs), Ordering::Relaxed);
    let _ = env.store.save_segments(job.id, &segs);
    env.info(
        job.id,
        format!(
            "start: {name}  {} bytes, {} segment{}{}",
            size,
            segs.len(),
            if segs.len() == 1 { "" } else { "s" },
            if probe.ranges || size == 0 {
                ""
            } else {
                " (no range support: single stream)"
            }
        ),
    );
    prog.phase.store(1, Ordering::Relaxed);

    let want = if probe.ranges {
        job.segments.max(1) as usize
    } else {
        1
    };
    let sh = Arc::new(Shared {
        env: env.clone(),
        url: probe.final_url.clone(),
        job: job.clone(),
        file: Arc::clone(&file),
        slots: Mutex::new(
            segs.into_iter()
                .map(|seg| Slot {
                    seg,
                    run: Run::Idle,
                    retries: 0,
                })
                .collect(),
        ),
        ranges: probe.ranges,
        want,
        prog: Arc::clone(&prog),
        cancel: cancel.child_token(),
        fatal: Mutex::new(None),
    });

    // Speed meter and periodic checkpoint.
    let ticker = {
        let sh = Arc::clone(&sh);
        tokio::spawn(async move {
            let mut last = sh.prog.done.load(Ordering::Relaxed);
            let mut n = 0u32;
            loop {
                sleep(Duration::from_millis(500)).await;
                let now = sh.prog.done.load(Ordering::Relaxed);
                let inst = now.saturating_sub(last) as f64 * 2.0;
                let prev = sh.prog.speed.load(Ordering::Relaxed) as f64;
                let sm = if prev == 0.0 {
                    inst
                } else {
                    0.6 * prev + 0.4 * inst
                };
                sh.prog.speed.store(sm as u64, Ordering::Relaxed);
                last = now;
                n += 1;
                if n.is_multiple_of(2) {
                    sh.checkpoint().await;
                }
            }
        })
    };

    let workers: Vec<_> = (0..sh.want.max(1))
        .map(|_| tokio::spawn(Arc::clone(&sh).worker()))
        .collect();
    for w in workers {
        let _ = w.await;
    }
    ticker.abort();
    sh.prog.speed.store(0, Ordering::Relaxed);
    sh.checkpoint().await;

    let fatal = sh.fatal.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(m) = fatal {
        return Outcome::Failed(m);
    }
    if cancel.is_cancelled() {
        return Outcome::Cancelled;
    }
    let done = segment::done_total(&sh.segments());
    if done != size {
        return Outcome::Failed(format!("internal size mismatch ({done} of {size} bytes)"));
    }
    prog.phase.store(2, Ordering::Relaxed);
    finalize(&env, &job, &probe, &file, &part).await
}

/// Servers that send no `Content-Length`: one sequential stream, no resume.
async fn stream_unknown_size(
    env: JobEnv,
    job: Job,
    probe: Probe,
    file: Arc<File>,
    part: PathBuf,
    cancel: CancellationToken,
    prog: Arc<Progress>,
) -> Outcome {
    env.info(
        job.id,
        format!(
            "start: {}  unknown size, single stream",
            job.out_path.rsplit('/').next().unwrap_or("")
        ),
    );
    prog.phase.store(1, Ordering::Relaxed);
    let mut attempt = 0;
    loop {
        let _ = file.set_len(0);
        prog.done.store(0, Ordering::Relaxed);
        let _conn = ConnGuard::new(&prog.connections);
        let result: Result<u64, FetchOrStop> = async {
            let resp = tokio::select! {
                _ = cancel.cancelled() => return Err(FetchOrStop::Cancelled),
                r = http::request(&env.client, &job, &env.settings.user_agent, &probe.final_url).send() =>
                    r.map_err(|e| FetchOrStop::Fetch(http::classify_error(&e)))?,
            };
            if let Some(e) = http::classify_status(resp.status()) {
                return Err(FetchOrStop::Fetch(e));
            }
            let mut stream = resp.bytes_stream();
            let mut pos = 0u64;
            loop {
                let next = tokio::select! {
                    _ = cancel.cancelled() => return Err(FetchOrStop::Cancelled),
                    n = stream.next() => n,
                };
                let Some(chunk) = next else { return Ok(pos) };
                let chunk = chunk.map_err(|e| FetchOrStop::Fetch(http::classify_error(&e)))?;
                env.limiter.acquire(chunk.len()).await;
                let (f, data, at) = (Arc::clone(&file), chunk.clone(), pos);
                tokio::task::spawn_blocking(move || f.write_all_at(&data, at))
                    .await
                    .map_err(|e| FetchOrStop::Io(e.to_string()))?
                    .map_err(|e| FetchOrStop::Io(e.to_string()))?;
                pos += chunk.len() as u64;
                prog.done.store(pos, Ordering::Relaxed);
            }
        }
        .await;
        match result {
            Ok(total) => {
                prog.size.store(total, Ordering::Relaxed);
                let mut j = job.clone();
                j.size = Some(total);
                let _ = env.store.set_meta(&j);
                return finalize(&env, &j, &probe, &file, &part).await;
            }
            Err(FetchOrStop::Cancelled) => return Outcome::Cancelled,
            Err(FetchOrStop::Io(m)) => return Outcome::Failed(format!("write error: {m}")),
            Err(FetchOrStop::Fetch(FetchError::Permanent(m))) => return Outcome::Failed(m),
            Err(FetchOrStop::Fetch(FetchError::Transient(m))) => {
                attempt += 1;
                if attempt > env.settings.retries {
                    return Outcome::Failed(m);
                }
                env.info(
                    job.id,
                    format!(
                        "retry: stream ({m}), attempt {attempt}/{}",
                        env.settings.retries
                    ),
                );
                if !sleep_or_cancel(backoff(attempt), &cancel).await {
                    return Outcome::Cancelled;
                }
            }
        }
    }
}

/// fsync, rename `.part` to the final name, verify the checksum, keep the server's mtime.
async fn finalize(
    env: &JobEnv,
    job: &Job,
    probe: &Probe,
    file: &Arc<File>,
    part: &std::path::Path,
) -> Outcome {
    let (f, part, dest) = (
        Arc::clone(file),
        part.to_path_buf(),
        PathBuf::from(&job.out_path),
    );
    let expect = job.sha256.clone();
    let lm = probe.last_modified.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
        f.sync_data()
            .map_err(|e| format!("cannot flush {}: {e}", part.display()))?;
        std::fs::rename(&part, &dest)
            .map_err(|e| format!("cannot rename {}: {e}", part.display()))?;
        if let Some(want) = expect {
            let mut h = Sha256::new();
            let mut r = File::open(&dest).map_err(|e| e.to_string())?;
            let mut buf = vec![0u8; 1 << 16];
            loop {
                let n = std::io::Read::read(&mut r, &mut buf).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            let got: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
            if !got.eq_ignore_ascii_case(&want) {
                return Err(format!(
                    "checksum mismatch (got {}..., expected {}...)",
                    &got[..16],
                    &want[..want.len().min(16)]
                ));
            }
        }
        if let Some(t) = lm
            .as_deref()
            .and_then(|s| httpdate::parse_http_date(s).ok())
            && let Ok(f) = std::fs::OpenOptions::new().write(true).open(&dest)
        {
            let _ = f.set_modified(t);
        }
        Ok(())
    })
    .await;
    match result {
        Ok(Ok(())) => {
            let _ = env.store.save_segments(job.id, &[]);
            Outcome::Done(PathBuf::from(&job.out_path))
        }
        Ok(Err(m)) => Outcome::Failed(m),
        Err(e) => Outcome::Failed(e.to_string()),
    }
}
