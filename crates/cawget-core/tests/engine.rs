//! End-to-end tests of the engine against a local HTTP/HTTPS server: real sockets, real files,
//! real SQLite. Each test starts its own server, so tests are independent and run in parallel.

use cawget_core::secrets::MemorySecretStore;
use cawget_core::{AddRequest, Event, Manager, RunOptions, RunSummary, State, Store};
use cawget_testserver::{self as server, TestServer, expected_sha, sha_file};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

const MIB: u64 = 1 << 20;

struct Env {
    _tmp: tempfile::TempDir,
    dl: PathBuf,
    db: PathBuf,
}

impl Env {
    fn new() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let dl = tmp.path().join("dl");
        std::fs::create_dir_all(&dl).unwrap();
        let db = tmp.path().join("s.db");
        Env { _tmp: tmp, dl, db }
    }
    /// A manager with an in-memory secret store: tests never touch the real keyring.
    fn manager(&self) -> Manager {
        Manager::with_store_and_secrets_at(
            Store::open(&self.db).unwrap(),
            Arc::new(MemorySecretStore::new()),
            Some(&self.db),
        )
    }
    fn files(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(&self.dl)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }
    fn req(&self, url: String) -> AddRequest {
        AddRequest {
            url,
            dir: Some(self.dl.to_string_lossy().into_owned()),
            ..Default::default()
        }
    }
}

async fn run_all(m: &Manager) -> RunSummary {
    tokio::time::timeout(
        Duration::from_secs(60),
        m.run(RunOptions::default(), CancellationToken::new()),
    )
    .await
    .expect("run() did not finish in 60 s")
    .expect("run() failed")
}

/// Collects events until the returned handle is dropped.
fn collect(m: &Manager) -> std::sync::Arc<std::sync::Mutex<Vec<Event>>> {
    let out = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let (o, mut rx) = (out.clone(), m.subscribe());
    tokio::spawn(async move {
        while let Ok(e) = rx.recv().await {
            o.lock().unwrap().push(e);
        }
    });
    out
}

fn infos(ev: &std::sync::Arc<std::sync::Mutex<Vec<Event>>>) -> Vec<String> {
    ev.lock()
        .unwrap()
        .iter()
        .filter_map(|e| {
            if let Event::Info { message, .. } = e {
                Some(message.clone())
            } else {
                None
            }
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn multi_segment_download_assembles_exact_file() {
    let (srv, env) = (server::start().await, Env::new());
    let m = env.manager();
    let size = 24 * MIB;
    let mut r = env.req(srv.url(&format!("/data/{size}?bps={}", 4 * MIB)));
    r.segments = Some(6);
    let id = m.add(r).unwrap();
    let s = run_all(&m).await;
    assert_eq!((s.done, s.failed), (1, 0));
    let f = env.dl.join(size.to_string());
    assert_eq!(std::fs::metadata(&f).unwrap().len(), size);
    assert_eq!(
        sha_file(&f),
        expected_sha(size),
        "content must be byte-exact"
    );
    assert_eq!(env.files(), vec![size.to_string()], "no .part left behind");
    let ranges = srv.state.data_ranges();
    assert!(
        ranges.len() >= 6,
        "expected >= 6 range requests, got {}",
        ranges.len()
    );
    assert!(
        srv.state.peak() >= 4,
        "only {} parallel transfers",
        srv.state.peak()
    );
    let served = srv.state.bytes_served();
    assert!(
        (size..=size + 2 * MIB).contains(&served),
        "served {served} bytes for a {size} byte file"
    );
    assert_eq!(m.get(id).unwrap().job.state, State::Done);
    assert!(
        m.store().load_segments(id).unwrap().is_empty(),
        "segments are dropped when done"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn https_with_custom_ca_and_rejection_without() {
    let srv = server::start_tls().await;
    let env = Env::new();
    let ca = env.dl.parent().unwrap().join("ca.pem");
    std::fs::write(&ca, srv.cert_pem.as_ref().unwrap()).unwrap();
    let m = env.manager();
    m.set_setting("cacert", ca.to_str().unwrap()).unwrap();
    m.add(env.req(srv.url("/data/6291456"))).unwrap();
    let s = run_all(&m).await;
    assert_eq!((s.done, s.failed), (1, 0));
    assert_eq!(sha_file(&env.dl.join("6291456")), expected_sha(6 * MIB));

    // Without the CA the same server must be refused, and quickly (not retried for minutes).
    let env2 = Env::new();
    let m2 = env2.manager();
    let id = m2.add(env2.req(srv.url("/data/1000"))).unwrap();
    let t0 = Instant::now();
    let s = run_all(&m2).await;
    assert_eq!((s.done, s.failed), (0, 1));
    assert!(
        t0.elapsed() < Duration::from_secs(5),
        "certificate errors must not be retried"
    );
    let j = m2.get(id).unwrap().job;
    assert_eq!(j.state, State::Failed);
    assert!(j.error.unwrap().to_lowercase().contains("certificate"));
    assert!(env2.files().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn server_without_ranges_falls_back_to_one_stream() {
    let (srv, env) = (server::start().await, Env::new());
    let m = env.manager();
    let ev = collect(&m);
    let mut r = env.req(srv.url("/norange/3145728"));
    r.segments = Some(8);
    m.add(r).unwrap();
    assert_eq!(run_all(&m).await.done, 1);
    assert_eq!(sha_file(&env.dl.join("3145728")), expected_sha(3 * MIB));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(infos(&ev).iter().any(|m| m.contains("single stream")));
    assert!(srv.state.log().iter().all(|r| r.status != 206));
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_length_chunked_stream() {
    let (srv, env) = (server::start().await, Env::new());
    let m = env.manager();
    let id = m.add(env.req(srv.url("/chunked/2097152"))).unwrap();
    assert_eq!(run_all(&m).await.done, 1);
    assert_eq!(sha_file(&env.dl.join("2097152")), expected_sha(2 * MIB));
    assert_eq!(
        m.get(id).unwrap().job.size,
        Some(2 * MIB),
        "size learned from the stream end"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn redirects_names_and_explicit_output() {
    let (srv, env) = (server::start().await, Env::new());
    let m = env.manager();
    m.add(env.req(srv.url("/redirect/1000000"))).unwrap();
    m.add(env.req(srv.url("/cd/500000"))).unwrap();
    assert_eq!(run_all(&m).await.done, 2);
    m.add(env.req(srv.url("/cd/500000"))).unwrap();
    let out = env.dl.join("explicit.dat");
    m.add(AddRequest {
        url: srv.url("/data/1234"),
        output: Some(out.to_string_lossy().into_owned()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(run_all(&m).await.done, 2);
    assert_eq!(
        env.files(),
        vec![
            "1000000",
            "explicit.dat",
            "report final (1).bin",
            "report final.bin"
        ]
    );
    assert_eq!(sha_file(&env.dl.join("1000000")), expected_sha(1_000_000));
    assert_eq!(std::fs::metadata(out).unwrap().len(), 1234);
}

#[tokio::test(flavor = "multi_thread")]
async fn transient_server_errors_are_retried() {
    let (srv, env) = (server::start().await, Env::new());
    let m = env.manager();
    let ev = collect(&m);
    m.add(env.req(srv.url("/flaky/1500000"))).unwrap();
    assert_eq!(run_all(&m).await.done, 1);
    assert_eq!(sha_file(&env.dl.join("1500000")), expected_sha(1_500_000));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(infos(&ev).iter().filter(|m| m.starts_with("retry")).count() >= 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn http_errors_fail_cleanly_and_bad_urls_are_refused() {
    let (srv, env) = (server::start().await, Env::new());
    let m = env.manager();
    let id = m.add(env.req(srv.url("/missing/file.bin"))).unwrap();
    let s = run_all(&m).await;
    assert_eq!((s.done, s.failed), (0, 1));
    let j = m.get(id).unwrap().job;
    assert_eq!(j.state, State::Failed);
    assert_eq!(j.error.as_deref(), Some("HTTP 404"));
    assert!(env.files().is_empty());
    assert!(m.add(env.req("ftp://example.org/x".into())).is_err());
    assert!(
        m.add(AddRequest {
            url: srv.url("/data/1"),
            headers: vec!["no colon".into()],
            ..Default::default()
        })
        .is_err()
    );
    assert!(
        m.add(AddRequest {
            url: srv.url("/data/1"),
            sha256: Some("zz".into()),
            ..Default::default()
        })
        .is_err()
    );
    // A failed job can be put back in the queue.
    m.resume(id).unwrap();
    assert_eq!(m.get(id).unwrap().job.state, State::Queued);
}

#[tokio::test(flavor = "multi_thread")]
async fn checksum_verification() {
    let (srv, env) = (server::start().await, Env::new());
    let m = env.manager();
    let mut ok = env.req(srv.url("/data/300000"));
    ok.sha256 = Some(expected_sha(300_000));
    m.add(ok).unwrap();
    assert_eq!(run_all(&m).await.done, 1);

    let env2 = Env::new();
    let m2 = env2.manager();
    let mut bad = env2.req(srv.url("/data/300000"));
    bad.sha256 = Some("0".repeat(64));
    let id = m2.add(bad).unwrap();
    let s = run_all(&m2).await;
    assert_eq!(s.failed, 1);
    assert!(
        m2.get(id)
            .unwrap()
            .job
            .error
            .unwrap()
            .contains("checksum mismatch")
    );
}

/// Starts the scheduler, lets it run for `after`, then stops it the requested way.
async fn start_then_stop(m: &Manager, after: Duration, graceful: bool) -> Option<RunSummary> {
    let token = CancellationToken::new();
    let (mm, t) = (m.clone(), token.clone());
    let handle = tokio::spawn(async move { mm.run(RunOptions::default(), t).await.unwrap() });
    tokio::time::sleep(after).await;
    if graceful {
        token.cancel();
        Some(handle.await.unwrap())
    } else {
        handle.abort(); // no cleanup runs: like kill -9 as far as the saved state goes
        let _ = handle.await;
        None
    }
}

async fn interrupt_and_resume(
    graceful: bool,
    bump: bool,
) -> (u64, u64, Vec<String>, Env, TestServer) {
    let (srv, env) = (server::start().await, Env::new());
    let size = 16 * MIB;
    let path = if bump {
        format!("/ver/{size}?bps={}", 1_500_000)
    } else {
        format!("/data/{size}?bps={}", MIB)
    };
    let m = env.manager();
    let mut r = env.req(srv.url(&path));
    r.segments = Some(4);
    let id = m.add(r).unwrap();
    let summary = start_then_stop(&m, Duration::from_millis(2600), graceful).await;
    if graceful {
        assert!(summary.unwrap().interrupted);
        assert_eq!(
            m.get(id).unwrap().job.state,
            State::Queued,
            "graceful stop re-queues"
        );
    }
    drop(m);
    let parts: Vec<_> = env
        .files()
        .into_iter()
        .filter(|f| f.ends_with(".part"))
        .collect();
    assert_eq!(
        parts.len(),
        1,
        "partial file must remain: {:?}",
        env.files()
    );
    let before = env.manager().store().done_bytes(id).unwrap();
    assert!(before > 0 && before < size, "progress not saved: {before}");
    if bump {
        srv.state.bump();
    }
    srv.state.reset().await;

    // "Restart the program": a brand new Manager on the same database.
    let m2 = env.manager();
    let ev = collect(&m2);
    let s = run_all(&m2).await;
    assert_eq!((s.done, s.failed), (1, 0));
    let f = env.dl.join(size.to_string());
    assert_eq!(sha_file(&f), expected_sha(size));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let served = srv.state.bytes_served();
    (before, served, infos(&ev), env, srv)
}

#[tokio::test(flavor = "multi_thread")]
async fn resume_after_graceful_shutdown_fetches_only_the_rest() {
    let size = 16 * MIB;
    let (before, served, info, _e, _s) = interrupt_and_resume(true, false).await;
    assert!(info.iter().any(|m| m.starts_with("resume")), "{info:?}");
    assert!(
        served <= size - before + MIB,
        "re-fetched too much: {served} B for {} B left",
        size - before
    );
    assert!(
        served + MIB >= size - before,
        "fetched too little: {served}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn resume_after_hard_abort_uses_the_last_checkpoint() {
    let size = 16 * MIB;
    let (before, served, info, _e, _s) = interrupt_and_resume(false, false).await;
    assert!(info.iter().any(|m| m.starts_with("resume")), "{info:?}");
    assert!(
        served <= size - before + MIB,
        "re-fetched too much: {served} B for {} B left",
        size - before
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn remote_change_while_stopped_restarts_from_scratch() {
    let size = 16 * MIB;
    let (_before, served, info, _e, _s) = interrupt_and_resume(true, true).await;
    assert!(info.iter().any(|m| m.starts_with("restart")), "{info:?}");
    assert!(
        served >= size,
        "a changed file must be fetched in full, got {served}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn work_stealing_splits_the_slow_segment() {
    let (srv, env) = (server::start().await, Env::new());
    let m = env.manager();
    m.set_setting("min_segment_size", "256k").unwrap();
    let size = 16 * MIB;
    let mut r = env.req(srv.url(&format!("/data/{size}?slow0={}", 512 * 1024)));
    r.segments = Some(2);
    m.add(r).unwrap();
    assert_eq!(run_all(&m).await.done, 1);
    assert_eq!(sha_file(&env.dl.join(size.to_string())), expected_sha(size));
    let n = srv.state.data_ranges().len();
    assert!(n > 2, "no extra segments were created ({n} ranges)");
}

#[tokio::test(flavor = "multi_thread")]
async fn pause_and_resume_on_a_running_engine() {
    let (srv, env) = (server::start().await, Env::new());
    let m = env.manager();
    let size = 10 * MIB;
    let mut r = env.req(srv.url(&format!("/data/{size}?bps={}", MIB)));
    r.segments = Some(2);
    let id = m.add(r).unwrap();
    let token = CancellationToken::new();
    let (mm, t) = (m.clone(), token.clone());
    let run = tokio::spawn(async move { mm.run(RunOptions::default(), t).await.unwrap() });
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(m.get(id).unwrap().done > 0, "live progress is visible");
    m.pause(id).unwrap();
    let s = tokio::time::timeout(Duration::from_secs(5), run)
        .await
        .expect("engine must stop after pausing")
        .unwrap();
    assert_eq!((s.done, s.interrupted), (0, false));
    let v = m.get(id).unwrap();
    assert_eq!(v.job.state, State::Paused);
    assert!(v.done > 0 && v.done < size);
    assert!(m.pause(id).is_err(), "pausing a paused job is an error");
    m.resume(id).unwrap();
    assert_eq!(run_all(&m).await.done, 1);
    assert_eq!(sha_file(&env.dl.join(size.to_string())), expected_sha(size));
}

#[tokio::test(flavor = "multi_thread")]
async fn another_process_can_pause_through_the_database() {
    let (srv, env) = (server::start().await, Env::new());
    let m = env.manager();
    let mut r = env.req(srv.url(&format!("/data/{}?bps={}", 8 * MIB, MIB)));
    r.segments = Some(1); // 8 MiB at 1 MiB/s: long enough to be interrupted
    let id = m.add(r).unwrap();
    let (mm, t) = (m.clone(), CancellationToken::new());
    let run = tokio::spawn(async move { mm.run(RunOptions::default(), t).await.unwrap() });
    tokio::time::sleep(Duration::from_millis(1200)).await;
    env.manager().pause(id).unwrap(); // a separate Manager = a separate CLI invocation
    tokio::time::timeout(Duration::from_secs(6), run)
        .await
        .expect("engine must notice the change")
        .unwrap();
    assert_eq!(m.get(id).unwrap().job.state, State::Paused);
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_a_running_job_deletes_partial_data() {
    let (srv, env) = (server::start().await, Env::new());
    let m = env.manager();
    let id = m
        .add(env.req(srv.url(&format!("/data/{}?bps={}", 8 * MIB, 512 * 1024))))
        .unwrap();
    let (mm, t) = (m.clone(), CancellationToken::new());
    let run = tokio::spawn(async move { mm.run(RunOptions::default(), t).await.unwrap() });
    tokio::time::sleep(Duration::from_millis(1500)).await;
    m.remove(id, false).unwrap();
    tokio::time::timeout(Duration::from_secs(6), run)
        .await
        .expect("engine must finish")
        .unwrap();
    assert!(m.list().unwrap().is_empty());
    assert!(env.files().is_empty(), "leftovers: {:?}", env.files());
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrency_limit_is_respected() {
    let (srv, env) = (server::start().await, Env::new());
    let m = env.manager();
    m.set_setting("max_jobs", "2").unwrap();
    for i in 0..4 {
        let mut r = env.req(srv.url(&format!("/data/{}?bps={}", 2 * MIB + i, 2 * MIB)));
        r.segments = Some(1);
        m.add(r).unwrap();
    }
    assert_eq!(run_all(&m).await.done, 4);
    assert_eq!(srv.state.peak(), 2, "max_jobs=2 with one segment each");
    assert_eq!(env.files().len(), 4);
}

#[tokio::test(flavor = "multi_thread")]
async fn scheduled_job_waits_for_its_time() {
    let (srv, env) = (server::start().await, Env::new());
    let m = env.manager();
    let ev = collect(&m);
    let at = cawget_core::util::unix_now() + 3;
    let mut r = env.req(srv.url("/data/200000"));
    r.not_before = at;
    m.add(r).unwrap();
    let t0 = Instant::now();
    assert_eq!(run_all(&m).await.done, 1);
    assert!(
        t0.elapsed() >= Duration::from_millis(1800),
        "started after {:?}",
        t0.elapsed()
    );
    assert!(infos(&ev).iter().any(|m| m.contains("waiting until")));
}

#[tokio::test(flavor = "multi_thread")]
async fn daily_window_holds_jobs_back() {
    let (srv, env) = (server::start().await, Env::new());
    let m = env.manager();
    // A window that is closed now and opens in the future: nothing may start, and the engine
    // (not in daemon mode) must wait rather than exit.
    let now = cawget_core::util::local_minute_of_day(cawget_core::util::unix_now());
    let (a, b) = ((now + 120) % 1440, (now + 180) % 1440);
    m.set_setting(
        "window",
        &format!("{:02}:{:02}-{:02}:{:02}", a / 60, a % 60, b / 60, b % 60),
    )
    .unwrap();
    let id = m.add(env.req(srv.url("/data/1000"))).unwrap();
    let token = CancellationToken::new();
    let (mm, t) = (m.clone(), token.clone());
    let run = tokio::spawn(async move { mm.run(RunOptions::default(), t).await.unwrap() });
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(
        !run.is_finished(),
        "engine must keep waiting for the window"
    );
    assert_eq!(m.get(id).unwrap().job.state, State::Queued);
    assert!(
        srv.state.log().is_empty(),
        "no request may be made outside the window"
    );
    token.cancel();
    assert!(run.await.unwrap().interrupted);
}

#[tokio::test(flavor = "multi_thread")]
async fn speed_limit_throttles_total_throughput() {
    let (srv, env) = (server::start().await, Env::new());
    let m = env.manager();
    m.set_setting("max_speed", "1M").unwrap();
    let mut r = env.req(srv.url("/data/3145728"));
    r.segments = Some(4);
    m.add(r).unwrap();
    let t0 = Instant::now();
    assert_eq!(run_all(&m).await.done, 1);
    let secs = t0.elapsed().as_secs_f64();
    assert!(
        (2.0..8.0).contains(&secs),
        "3 MiB at 1 MiB/s took {secs:.1}s"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn custom_headers_reach_every_request() {
    let (srv, env) = (server::start().await, Env::new());
    let m = env.manager();
    let mut r = env.req(srv.url(&format!("/data/{}", 3 * MIB)));
    r.segments = Some(2);
    r.headers = vec!["X-Test: abc123".into()];
    r.referer = Some("http://ref.example/page".into());
    r.cookie = Some("sid=42; a=b".into());
    r.user_agent = Some("unit-agent/9".into());
    m.add(r).unwrap();
    assert_eq!(run_all(&m).await.done, 1);
    let log = srv.state.log();
    assert!(
        log.len() >= 3,
        "probe + 2 segments expected, got {}",
        log.len()
    );
    for q in log {
        assert_eq!(q.x_test.as_deref(), Some("abc123"));
        assert_eq!(q.referer.as_deref(), Some("http://ref.example/page"));
        assert_eq!(q.cookie.as_deref(), Some("sid=42; a=b"));
        assert_eq!(q.user_agent.as_deref(), Some("unit-agent/9"));
        assert!(q.accept_encoding.is_none(), "ranges need the raw entity");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn empty_file_and_state_housekeeping() {
    let (srv, env) = (server::start().await, Env::new());
    let m = env.manager();
    let id = m.add(env.req(srv.url("/data/0"))).unwrap();
    assert_eq!(run_all(&m).await.done, 1);
    assert_eq!(std::fs::metadata(env.dl.join("0")).unwrap().len(), 0);
    // Completed jobs can be cleared (file stays) or removed with their file.
    assert_eq!(m.clear_finished().unwrap(), 1);
    assert!(env.dl.join("0").exists());
    assert!(m.get(id).is_err());
    let id2 = m.add(env.req(srv.url("/data/10"))).unwrap();
    run_all(&m).await;
    m.remove(id2, true).unwrap();
    assert!(!env.dl.join("10").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn crashed_active_jobs_are_picked_up_again() {
    let (srv, env) = (server::start().await, Env::new());
    let m = env.manager();
    let id = m.add(env.req(srv.url("/data/50000"))).unwrap();
    m.store().set_state(id, State::Active, None).unwrap(); // as left behind by a crash
    drop(m);
    let m2 = env.manager();
    assert_eq!(run_all(&m2).await.done, 1);
    assert_eq!(sha_file(&env.dl.join("50000")), expected_sha(50_000));
    let _ = Path::new("");
    let _ = Store::open_in_memory().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn only_one_scheduler_may_work_on_a_database() {
    let (srv, env) = (server::start().await, Env::new());
    let m = env.manager();
    m.add(env.req(srv.url(&format!("/data/{}?bps={}", 4 * MIB, MIB))))
        .unwrap();
    let (mm, t) = (m.clone(), CancellationToken::new());
    let first = tokio::spawn({
        let t = t.clone();
        async move { mm.run(RunOptions::default(), t).await.unwrap() }
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
    let second = env
        .manager()
        .run(RunOptions::default(), CancellationToken::new())
        .await;
    assert!(
        matches!(second, Err(cawget_core::Error::Busy)),
        "second scheduler must be refused: {second:?}"
    );
    // Queue edits from the second handle still work while the first runs.
    assert!(env.manager().list().unwrap().len() == 1);
    t.cancel();
    first.await.unwrap();
    // After the first one has gone, the lock is free again.
    let again = env
        .manager()
        .run(
            RunOptions {
                only_id: Some(999),
                daemon: false,
            },
            CancellationToken::new(),
        )
        .await;
    assert!(again.is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn probe_reports_size_name_and_resume_support() {
    let (srv, env) = (server::start().await, Env::new());
    let m = env.manager();
    let req = |p: &str| AddRequest {
        url: srv.url(p),
        ..Default::default()
    };
    let p = m.probe_url(&req("/data/12345")).await.unwrap();
    assert_eq!(
        (p.size, p.resumable, p.file_name.as_str()),
        (Some(12345), true, "12345")
    );
    let p = m.probe_url(&req("/norange/777")).await.unwrap();
    assert_eq!((p.size, p.resumable), (Some(777), false));
    assert_eq!(
        m.probe_url(&req("/cd/10")).await.unwrap().file_name,
        "report final.bin"
    );
    let p = m.probe_url(&req("/redirect/99")).await.unwrap();
    assert!(p.final_url.ends_with("/data/99"), "{}", p.final_url);
    assert_eq!(
        m.probe_url(&req("/missing/x")).await.unwrap_err(),
        "HTTP 404"
    );
    assert!(m.probe_url(&req("ftp://x/y")).await.is_err());
    assert!(m.list().unwrap().is_empty(), "probing never adds a job");
}

#[tokio::test(flavor = "multi_thread")]
async fn basic_authorization_header_is_used_for_probe_and_download() {
    let (srv, env) = (server::start().await, Env::new());
    let m = env.manager();
    let size = 3 * MIB;
    let auth = cawget_core::http::basic_auth_header("user", "pa:ss wörd");
    let with = |u: String| AddRequest {
        url: u,
        headers: vec![auth.clone()],
        dir: Some(env.dl.to_string_lossy().into_owned()),
        ..Default::default()
    };
    assert_eq!(
        m.probe_url(&AddRequest {
            url: srv.url(&format!("/auth/{size}")),
            ..Default::default()
        })
        .await
        .unwrap_err(),
        "HTTP 401"
    );
    assert_eq!(
        m.probe_url(&with(srv.url(&format!("/auth/{size}"))))
            .await
            .unwrap()
            .size,
        Some(size)
    );
    let mut ok = with(srv.url(&format!("/auth/{size}")));
    ok.segments = Some(4);
    let id_ok = m.add(ok).unwrap();
    let id_bad = m.add(env.req(srv.url("/auth/1000"))).unwrap();
    let s = run_all(&m).await;
    assert_eq!((s.done, s.failed), (1, 1));
    assert_eq!(sha_file(&env.dl.join(size.to_string())), expected_sha(size));
    assert_eq!(
        m.get(id_bad).unwrap().job.error.as_deref(),
        Some("HTTP 401")
    );
    // 3 MiB at the 1 MiB minimum segment size = 3 ranged requests, each of which needed the header
    assert!(
        srv.state.data_ranges().len() >= 3,
        "segments carried the header too"
    );
    let _ = id_ok;
}

#[tokio::test(flavor = "multi_thread")]
async fn download_later_adds_a_paused_job_that_starts_only_when_resumed() {
    let (srv, env) = (server::start().await, Env::new());
    let m = env.manager();
    let mut r = env.req(srv.url("/data/4096"));
    r.paused = true;
    r.output = Some(env.dl.join("later.bin").to_string_lossy().into_owned());
    let id = m.add(r).unwrap();
    assert_eq!(
        run_all(&m).await,
        RunSummary::default(),
        "a paused job is not started"
    );
    assert!(srv.state.log().is_empty());
    m.resume(id).unwrap();
    assert_eq!(run_all(&m).await.done, 1);
    assert_eq!(
        std::fs::metadata(env.dl.join("later.bin")).unwrap().len(),
        4096
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn files_without_a_chosen_folder_go_to_their_category_folder() {
    let (srv, env) = (server::start().await, Env::new());
    let m = env.manager();
    let (default, archives) = (env.dl.join("default"), env.dl.join("archives"));
    m.set_setting("download_dir", default.to_str().unwrap())
        .unwrap();
    m.set_setting("dir_compressed", archives.to_str().unwrap())
        .unwrap();
    let plain = |p: &str| AddRequest {
        url: srv.url(p),
        ..Default::default()
    };
    m.add(plain("/data/1000/backup.tar.gz")).unwrap();
    m.add(plain("/data/1001/notes.txt")).unwrap(); // documents: no folder set -> default
    m.add(AddRequest {
        dir: Some(env.dl.join("chosen").to_string_lossy().into_owned()),
        ..plain("/data/1002/other.zip")
    })
    .unwrap();
    assert_eq!(run_all(&m).await.done, 3);
    assert_eq!(
        std::fs::metadata(archives.join("backup.tar.gz"))
            .unwrap()
            .len(),
        1000
    );
    assert_eq!(
        std::fs::metadata(default.join("notes.txt")).unwrap().len(),
        1001
    );
    assert_eq!(
        std::fs::metadata(env.dl.join("chosen/other.zip"))
            .unwrap()
            .len(),
        1002,
        "an explicit folder wins"
    );
    assert!(!archives.join("other.zip").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn manual_proxy_setting_routes_requests_through_the_proxy() {
    let (proxy, env) = (server::start().await, Env::new());
    let m = env.manager();
    m.set_setting("proxy", &proxy.base).unwrap();
    // The host does not exist: the download can only succeed through the proxy.
    let url = "http://cawget-proxy-test.invalid/data/5000".to_string();
    m.add(env.req(url)).unwrap();
    let s = run_all(&m).await;
    assert_eq!((s.done, s.failed), (1, 0));
    assert_eq!(sha_file(&env.dl.join("5000")), expected_sha(5000));
    assert!(
        !proxy.state.log().is_empty(),
        "the proxy received the requests"
    );
    // "none" turns the proxy off again: the unknown host then fails.
    m.set_setting("proxy", "none").unwrap();
    let id = m
        .add(env.req("http://cawget-proxy-test.invalid/data/6000".into()))
        .unwrap();
    m.set_setting("retries", "0").unwrap();
    assert_eq!(run_all(&m).await.failed, 1);
    assert!(m.get(id).unwrap().job.error.is_some());
}
