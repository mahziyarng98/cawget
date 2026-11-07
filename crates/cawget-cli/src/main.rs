//! `cawget-cli`: command line front end of the download manager (engine: cawget-core).

use cawget_core::redact::{mask_url, redact_text};
use cawget_core::util::{fmt_bytes, fmt_eta, parse_size, parse_when, unix_now};
use cawget_core::{AddRequest, Event, JobView, Manager, RunOptions, SETTINGS, State};
use clap::{Args, Parser, Subcommand};
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Parser)]
#[command(
    name = "cawget-cli",
    version,
    about = "Segmented HTTP/HTTPS download manager",
    propagate_version = true
)]
struct Cli {
    /// Database file (default: $CAWGET_DB or ~/.local/share/cawget/cawget.db; $JACKDAW_DB and $SEGGET_DB are
    /// deprecated aliases)
    #[arg(long, global = true, env = "CAWGET_DB")]
    db: Option<PathBuf>,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Queue downloads
    Add(AddArgs),
    /// Queue one download and fetch it now
    Get(AddArgs),
    /// Process the queue
    Run(RunArgs),
    /// Show all jobs
    #[command(alias = "ls")]
    List,
    /// Show one job with its segments
    Status { id: i64 },
    /// Stop a job, keeping its progress
    Pause { id: i64 },
    /// Queue a paused or failed job again
    Resume { id: i64 },
    /// Remove a job (partial data is deleted)
    Rm {
        id: i64,
        /// Also delete the finished file
        #[arg(long)]
        files: bool,
    },
    /// Forget finished jobs (files stay)
    Clean,
    /// Show or change settings
    Config {
        key: Option<String>,
        value: Option<String>,
        /// Reset the setting to its default
        #[arg(long)]
        unset: bool,
    },
}

#[derive(Args)]
struct AddArgs {
    #[arg(required = true)]
    urls: Vec<String>,
    /// Output file
    #[arg(short, long)]
    output: Option<String>,
    /// Output directory
    #[arg(short, long)]
    dir: Option<String>,
    /// Connections per file (1-64)
    #[arg(short, long)]
    segments: Option<u32>,
    /// Extra request header "Name: value" (repeatable)
    #[arg(short = 'H', long = "header")]
    headers: Vec<String>,
    #[arg(long)]
    referer: Option<String>,
    #[arg(long)]
    cookie: Option<String>,
    #[arg(short = 'A', long)]
    user_agent: Option<String>,
    /// Verify the finished file against this SHA-256
    #[arg(long)]
    sha256: Option<String>,
    /// Start later: +10m, +2h, HH:MM, "YYYY-MM-DD HH:MM", @epoch
    #[arg(long)]
    at: Option<String>,
    /// Add without queueing for start
    #[arg(long)]
    paused: bool,
}

#[derive(Args)]
struct RunArgs {
    /// Concurrent jobs
    #[arg(short = 'j', long)]
    jobs: Option<u32>,
    /// Total speed limit, e.g. 500k or 2M
    #[arg(short = 'l', long)]
    limit: Option<String>,
    /// Keep running and pick up new jobs
    #[arg(long)]
    daemon: bool,
    #[arg(short, long)]
    quiet: bool,
}

/// The database when none was given: a deprecated alias of `$CAWGET_DB` (`$JACKDAW_DB`, then `$SEGGET_DB`),
/// else the default location (copying a legacy database there on first use; the old file is never changed).
fn default_db() -> PathBuf {
    if let Some((p, var)) = cawget_core::paths::db_from_env() {
        if var != cawget_core::paths::DB_ENV {
            eprintln!(
                "cawget-cli: note: {var} is deprecated, use {}",
                cawget_core::paths::DB_ENV
            );
        }
        return p;
    }
    match cawget_core::paths::resolve_default_db() {
        Ok((path, cawget_core::Migration::Copied { from })) => {
            eprintln!(
                "cawget-cli: copied the download queue from {} to {} (the old file was not changed)",
                from.display(),
                path.display()
            );
            path
        }
        Ok((path, cawget_core::Migration::Deferred { old })) => {
            eprintln!(
                "cawget-cli: the old queue {} is in use by another process; using it in place for now",
                old.display()
            );
            path
        }
        Ok((path, _)) => path,
        Err(e) => {
            eprintln!(
                "cawget-cli: could not copy the old queue ({e}); starting with the new location"
            );
            cawget_core::paths::default_db_in(&cawget_core::paths::data_dir())
        }
    }
}

fn fail(msg: impl std::fmt::Display) -> ExitCode {
    eprintln!("cawget-cli: {}", redact_text(&msg.to_string()));
    ExitCode::from(1)
}

fn build_requests(a: &AddArgs) -> Result<Vec<AddRequest>, String> {
    let not_before = match &a.at {
        Some(w) => parse_when(w, unix_now()).map_err(|e| e.to_string())?,
        None => 0,
    };
    if a.output.is_some() && a.urls.len() > 1 {
        return Err("-o names a single file; give one URL or use -d".into());
    }
    Ok(a.urls
        .iter()
        .map(|u| AddRequest {
            url: u.clone(),
            output: a.output.clone(),
            dir: a.dir.clone(),
            segments: a.segments,
            headers: a.headers.clone(),
            referer: a.referer.clone(),
            cookie: a.cookie.clone(),
            user_agent: a.user_agent.clone(),
            sha256: a.sha256.clone(),
            not_before,
            paused: a.paused,
        })
        .collect())
}

// ---- progress display ----

struct Ui {
    mp: MultiProgress,
    tty: bool,
    quiet: bool,
    bars: HashMap<i64, ProgressBar>,
    tick: u32,
}

impl Ui {
    fn new(quiet: bool) -> Ui {
        let tty = std::io::stderr().is_terminal();
        let mp = MultiProgress::with_draw_target(if tty && !quiet {
            ProgressDrawTarget::stderr()
        } else {
            ProgressDrawTarget::hidden()
        });
        Ui {
            mp,
            tty,
            quiet,
            bars: HashMap::new(),
            tick: 0,
        }
    }

    fn line(&self, s: String) {
        if self.quiet {
            return;
        }
        let s = redact_text(&s);
        if self.tty {
            let _ = self.mp.println(s);
        } else {
            eprintln!("{s}");
        }
    }

    fn event(&self, e: Event) {
        match e {
            Event::Info { id, message } => {
                let (kind, rest) = message.split_once(": ").unwrap_or(("info", &message));
                if id == 0 {
                    self.line(message);
                } else {
                    self.line(format!("{kind:<8}[{id}] {rest}"));
                }
            }
            Event::Done { id, path } => self.line(format!("{:<8}[{id}] {path}", "done")),
            Event::Failed { id, error } => self.line(format!("{:<8}[{id}] {error}", "failed")),
        }
    }

    fn render(&mut self, views: &[JobView]) {
        self.tick += 1;
        if self.quiet {
            return;
        }
        let active: Vec<&JobView> = views
            .iter()
            .filter(|v| v.job.state == State::Active && v.connections > 0)
            .collect();
        if self.tty {
            let style = ProgressStyle::with_template("[{prefix}] {wide_msg}").unwrap();
            for v in &active {
                let bar = self.bars.entry(v.job.id).or_insert_with(|| {
                    let b = self.mp.add(ProgressBar::new(100));
                    b.set_style(style.clone());
                    b.set_prefix(v.job.id.to_string());
                    b
                });
                bar.set_message(status_line(v, 20));
            }
            let live: Vec<i64> = active.iter().map(|v| v.job.id).collect();
            self.bars.retain(|id, b| {
                let keep = live.contains(id);
                if !keep {
                    b.finish_and_clear();
                }
                keep
            });
        } else if self.tick.is_multiple_of(20) {
            for v in active {
                eprintln!("[{}] {}", v.job.id, status_line(v, 0));
            }
        }
    }
}

fn status_line(v: &JobView, bar_width: usize) -> String {
    let pct = v.fraction.map_or(0.0, |f| f * 100.0);
    let bar = if bar_width > 0 {
        let fill = (pct / 100.0 * bar_width as f64).round() as usize;
        format!("[{}{}] ", "#".repeat(fill), "-".repeat(bar_width - fill))
    } else {
        String::new()
    };
    let total = v.job.size.map_or("?".into(), |s| fmt_bytes(s as f64));
    let name: String = v.name.chars().take(24).collect();
    format!(
        "{name:<24} {bar}{pct:>5.1}%  {}/{total}  {}/s  ETA {}  conn {}",
        fmt_bytes(v.done as f64),
        fmt_bytes(v.speed as f64),
        fmt_eta(v.eta_secs.map(|s| s as f64)),
        v.connections
    )
}

async fn run_engine(m: Manager, opts: RunOptions, quiet: bool) -> ExitCode {
    let token = CancellationToken::new();
    {
        let t = token.clone();
        tokio::spawn(async move {
            let mut term =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = async { match term.as_mut() { Some(s) => { s.recv().await; } None => std::future::pending().await } } => {}
            }
            t.cancel();
        });
    }
    let mut ui = Ui::new(quiet);
    let mut rx = m.subscribe();
    let ui_task = {
        let (m, stop) = (m.clone(), CancellationToken::new());
        let stop2 = stop.clone();
        let handle = tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(250));
            loop {
                tokio::select! {
                    _ = stop2.cancelled() => break,
                    _ = tick.tick() => { if let Ok(v) = m.list() { ui.render(&v); } }
                    e = rx.recv() => match e {
                        Ok(e) => ui.event(e),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(_) => break,
                    },
                }
            }
            // events sent just before the scheduler returned
            while let Ok(e) = rx.try_recv() {
                ui.event(e);
            }
            for b in ui.bars.values() {
                b.finish_and_clear();
            }
        });
        (stop, handle)
    };
    let summary = match m.run(opts, token).await {
        Ok(s) => s,
        Err(e) => {
            ui_task.0.cancel();
            let _ = ui_task.1.await;
            return fail(e);
        }
    };
    tokio::time::sleep(Duration::from_millis(30)).await;
    ui_task.0.cancel();
    let _ = ui_task.1.await;
    if summary.interrupted {
        ExitCode::from(130)
    } else if summary.failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

// ---- listing ----

fn print_list(views: &[JobView]) {
    if views.is_empty() {
        println!("no jobs");
        return;
    }
    println!(
        "{:<4} {:<8} {:>6} {:>10}  NAME",
        "ID", "STATE", "DONE", "SIZE"
    );
    for v in views {
        let size = v.job.size.map_or("?".to_string(), |s| fmt_bytes(s as f64));
        let pct = if v.job.state == State::Done {
            "100%".to_string()
        } else {
            v.fraction
                .map_or("-".into(), |f| format!("{:.0}%", f * 100.0))
        };
        let mut line = format!(
            "{:<4} {:<8} {:>6} {:>10}  {}",
            v.job.id,
            v.job.state.as_str(),
            pct,
            size,
            v.name
        );
        if v.job.not_before > unix_now() && v.job.state == State::Queued {
            use chrono::TimeZone;
            if let Some(d) = chrono::Local.timestamp_opt(v.job.not_before, 0).single() {
                line.push_str(&format!("  (scheduled {})", d.format("%m-%d %H:%M")));
            }
        }
        if let Some(e) = &v.job.error {
            line.push_str(&format!("  [{}]", redact_text(e)));
        }
        println!("{line}");
    }
}

fn print_status(m: &Manager, id: i64) -> Result<(), String> {
    let v = m.get(id).map_err(|e| e.to_string())?;
    let j = &v.job;
    println!(
        "id:        {}\nurl:       {}\nstate:     {}",
        j.id,
        mask_url(&j.url),
        j.state.as_str()
    );
    println!(
        "output:    {}",
        if j.out_path.is_empty() || j.out_path.ends_with('/') {
            "(decided on first connect)"
        } else {
            &j.out_path
        }
    );
    if let Some(e) = &j.error {
        println!("error:     {}", redact_text(e));
    }
    if let Some(s) = j.size {
        println!("size:      {} ({s} bytes)", fmt_bytes(s as f64));
    }
    println!(
        "ranges:    {}",
        if j.size.is_none() {
            "unknown"
        } else if j.accept_ranges {
            "yes"
        } else {
            "no"
        }
    );
    if let Some(e) = &j.etag {
        println!("etag:      {e}");
    }
    let segs = m.store().load_segments(id).map_err(|e| e.to_string())?;
    if !segs.is_empty() {
        println!("segments:  {}", segs.len());
        for (i, s) in segs.iter().enumerate() {
            println!(
                "  #{i:<2}  {:>12} - {:<12}  {:>5.1}%",
                s.start,
                s.end,
                100.0 * s.done as f64 / s.len() as f64
            );
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let db = cli.db.unwrap_or_else(default_db);
    let m = match Manager::open(&db) {
        Ok(m) => m,
        Err(e) => return fail(format!("cannot open database {}: {e}", db.display())),
    };
    // Report what the plaintext→keyring migration did (or why nothing was migrated).
    if let Some(r) = m.keyring_migration()
        && !matches!(r, cawget_core::secrets::KeyringMigration::NotNeeded)
    {
        eprintln!("cawget-cli: {}", redact_text(&r.summary()));
    }
    match cli.command {
        Cmd::Add(a) => {
            let reqs = match build_requests(&a) {
                Ok(r) => r,
                Err(e) => return fail(e),
            };
            for r in reqs {
                let url = r.url.clone();
                match m.add(r) {
                    Ok(id) => println!("queued [{id}] {}", mask_url(&url)),
                    Err(e) => return fail(e),
                }
            }
            ExitCode::SUCCESS
        }
        Cmd::Get(a) => {
            if a.urls.len() != 1 {
                return fail("get takes exactly one URL (use add + run for several)");
            }
            let req = match build_requests(&a) {
                Ok(mut r) => r.remove(0),
                Err(e) => return fail(e),
            };
            let url = req.url.clone();
            let id = match m.add(req) {
                Ok(id) => id,
                Err(e) => return fail(e),
            };
            println!("queued [{id}] {}", mask_url(&url));
            run_engine(
                m,
                RunOptions {
                    only_id: Some(id),
                    daemon: false,
                },
                false,
            )
            .await
        }
        Cmd::Run(a) => {
            if let Some(j) = a.jobs
                && let Err(e) = m.set_setting("max_jobs", &j.to_string())
            {
                return fail(e);
            }
            if let Some(l) = &a.limit {
                if parse_size(l).is_err() {
                    return fail(format!("bad rate '{l}'"));
                }
                if let Err(e) = m.set_setting("max_speed", l) {
                    return fail(e);
                }
            }
            run_engine(
                m,
                RunOptions {
                    only_id: None,
                    daemon: a.daemon,
                },
                a.quiet,
            )
            .await
        }
        Cmd::List => match m.list() {
            Ok(v) => {
                print_list(&v);
                ExitCode::SUCCESS
            }
            Err(e) => fail(e),
        },
        Cmd::Status { id } => match print_status(&m, id) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => fail(e),
        },
        Cmd::Pause { id } => finish(m.pause(id), id, "paused"),
        Cmd::Resume { id } => finish(m.resume(id), id, "queued"),
        Cmd::Rm { id, files } => finish(m.remove(id, files), id, "removed"),
        Cmd::Clean => match m.clear_finished() {
            Ok(n) => {
                println!("removed {n} finished job(s)");
                ExitCode::SUCCESS
            }
            Err(e) => fail(e),
        },
        Cmd::Config { key, value, unset } => config(&m, key, value, unset),
    }
}

fn finish(r: cawget_core::Result<()>, id: i64, what: &str) -> ExitCode {
    match r {
        Ok(()) => {
            println!("[{id}] {what}");
            ExitCode::SUCCESS
        }
        Err(e) => fail(e),
    }
}

fn config(m: &Manager, key: Option<String>, value: Option<String>, unset: bool) -> ExitCode {
    let mut stored = m.store().all_settings().unwrap_or_default();
    // The proxy is shown as the manager displays it: a saved login only as `***@`.
    if stored.contains_key("proxy") {
        stored.insert("proxy".into(), m.settings().proxy);
    }
    match (key, value) {
        (None, _) => {
            for s in SETTINGS {
                let (v, is_default) = match stored.get(s.key) {
                    Some(v) => (v.as_str(), false),
                    None => (s.default, true),
                };
                println!(
                    "{:<18} {:<14} {}{}",
                    s.key,
                    v,
                    s.help,
                    if is_default { "  (default)" } else { "" }
                );
            }
            ExitCode::SUCCESS
        }
        (Some(k), None) if !unset => match SETTINGS.iter().find(|s| s.key == k) {
            Some(s) => {
                println!("{}", stored.get(&k).map_or(s.default, |v| v.as_str()));
                ExitCode::SUCCESS
            }
            None => {
                eprintln!("cawget-cli: unknown setting '{k}'");
                ExitCode::from(2)
            }
        },
        (Some(k), v) => {
            let value = if unset {
                String::new()
            } else {
                v.unwrap_or_default()
            };
            match m.set_setting(&k, &value) {
                Ok(()) => {
                    println!(
                        "{k} = {}",
                        if value.is_empty() {
                            "(default)".to_string()
                        } else {
                            mask_url(&value)
                        }
                    );
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("cawget-cli: {}", redact_text(&e.to_string()));
                    ExitCode::from(2)
                }
            }
        }
    }
}
