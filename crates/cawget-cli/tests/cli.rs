//! Tests that run the real `cawget-cli` binary as a separate process against a local server,
//! including a genuine SIGKILL in the middle of a download.

use cawget_testserver::{self as server, expected_sha, sha_file};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

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
    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_cawget-cli"));
        c.arg("--db")
            .arg(&self.db)
            .args(args)
            .stdin(Stdio::null())
            .kill_on_drop(true);
        c
    }
    async fn run(&self, args: &[&str]) -> (i32, String) {
        let out = tokio::time::timeout(Duration::from_secs(60), self.cmd(args).output())
            .await
            .expect("command timed out")
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (out.status.code().unwrap_or(-1), text)
    }
    fn files(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(&self.dl)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }
    fn dir(&self) -> &str {
        self.dl.to_str().unwrap()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn get_downloads_a_file_and_exits_zero() {
    let (srv, env) = (server::start().await, Env::new());
    let url = srv.url(&format!("/data/{}", 6 * MIB));
    let (code, out) = env.run(&["get", "-s", "4", "-d", env.dir(), &url]).await;
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("done"), "{out}");
    assert_eq!(
        sha_file(&env.dl.join((6 * MIB).to_string())),
        expected_sha(6 * MIB)
    );
    let (code, out) = env.run(&["list"]).await;
    assert_eq!(code, 0);
    assert!(out.contains("done") && out.contains("100%"), "{out}");
    let (code, out) = env.run(&["status", "1"]).await;
    assert!(
        code == 0 && out.contains("state:     done") && out.contains("ranges:    yes"),
        "{out}"
    );
    let (_, out) = env.run(&["clean"]).await;
    assert!(out.contains("removed 1"), "{out}");
}

#[tokio::test(flavor = "multi_thread")]
async fn exit_codes_for_failures_and_bad_input() {
    let (srv, env) = (server::start().await, Env::new());
    let (code, out) = env
        .run(&["get", "-d", env.dir(), &srv.url("/missing/x")])
        .await;
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("HTTP 404"), "{out}");
    assert!(env.files().is_empty());
    assert_ne!(env.run(&["add", "ftp://example.org/x"]).await.0, 0);
    assert_eq!(env.run(&["config", "segments", "999"]).await.0, 2);
    assert_eq!(env.run(&["config", "nope"]).await.0, 2);
    assert_eq!(env.run(&["status", "42"]).await.0, 1);
    assert_eq!(
        env.run(&["add", "--at", "someday", "http://x/y"]).await.0,
        1
    );
    assert_eq!(
        env.run(&["add", "-o", "a", "http://x/1", "http://x/2"])
            .await
            .0,
        1,
        "one output name for two URLs"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn config_roundtrip_across_processes() {
    let env = Env::new();
    assert_eq!(env.run(&["config", "segments", "3"]).await.0, 0);
    assert_eq!(env.run(&["config", "max_speed", "2M"]).await.0, 0);
    assert_eq!(env.run(&["config", "window", "01:00-06:30"]).await.0, 0);
    let (_, v) = env.run(&["config", "segments"]).await;
    assert_eq!(v.trim(), "3");
    let (_, all) = env.run(&["config"]).await;
    assert!(
        all.lines()
            .any(|l| l.starts_with("max_speed") && l.contains("2M") && !l.contains("(default)")),
        "{all}"
    );
    assert!(
        all.lines()
            .any(|l| l.starts_with("retries") && l.contains("(default)")),
        "{all}"
    );
    assert_eq!(env.run(&["config", "--unset", "segments"]).await.0, 0);
    assert_eq!(env.run(&["config", "segments"]).await.1.trim(), "8");
}

async fn resume_scenario(sig: &str, expect_code: Option<i32>) {
    let (srv, env) = (server::start().await, Env::new());
    let size = 16 * MIB;
    let url = srv.url(&format!("/data/{size}?bps={MIB}"));
    assert_eq!(
        env.run(&["add", "-s", "4", "-d", env.dir(), &url]).await.0,
        0
    );

    let mut child = env
        .cmd(&["run"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(2800)).await;
    let pid = child.id().unwrap().to_string();
    let _ = Command::new("kill")
        .args([&format!("-{sig}"), &pid])
        .status()
        .await
        .unwrap();
    let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .expect("process must end")
        .unwrap();
    match expect_code {
        Some(c) => assert_eq!(status.code(), Some(c)),
        None => assert!(status.code().is_none(), "killed by signal: {status:?}"),
    }
    let parts: Vec<_> = env
        .files()
        .into_iter()
        .filter(|f| f.ends_with(".part"))
        .collect();
    assert_eq!(
        parts.len(),
        1,
        "partial file expected, got {:?}",
        env.files()
    );
    let (_, listing) = env.run(&["list"]).await;
    assert!(
        listing.contains("queued") || listing.contains("active"),
        "{listing}"
    );
    let (_, status_out) = env.run(&["status", "1"]).await;
    let saved: u64 = status_out
        .lines()
        .filter(|l| l.trim_start().starts_with('#'))
        .map(|l| {
            let nums: Vec<u64> = l
                .split(|c: char| !c.is_ascii_digit())
                .filter_map(|t| t.parse().ok())
                .collect();
            let pct: f64 = l
                .rsplit_once(' ')
                .and_then(|_| {
                    l.trim()
                        .trim_end_matches('%')
                        .rsplit(' ')
                        .next()?
                        .parse()
                        .ok()
                })
                .unwrap_or(0.0);
            (((nums[2] - nums[1] + 1) as f64) * pct / 100.0) as u64
        })
        .sum();
    assert!(
        saved > MIB,
        "progress must have been saved ({saved} B):\n{status_out}"
    );

    srv.state.reset().await;
    let (code, out) = env.run(&["run"]).await;
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("resume"), "{out}");
    assert_eq!(sha_file(&env.dl.join(size.to_string())), expected_sha(size));
    let served = srv.state.bytes_served();
    assert!(
        served < size - saved / 2 && served + 3 * MIB > size - saved,
        "second run fetched {served} of {size} (saved {saved})"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn resume_after_sigint_exit_code_130() {
    resume_scenario("INT", Some(130)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn resume_after_real_sigkill() {
    resume_scenario("KILL", None).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn pause_from_a_second_process_stops_the_first() {
    let (srv, env) = (server::start().await, Env::new());
    let url = srv.url(&format!("/data/{}?bps={}", 8 * MIB, MIB));
    assert_eq!(
        env.run(&["add", "-s", "1", "-d", env.dir(), &url]).await.0,
        0
    );
    let mut child = env.cmd(&["run", "-q"]).spawn().unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(env.run(&["pause", "1"]).await.0, 0);
    let st = tokio::time::timeout(Duration::from_secs(8), child.wait())
        .await
        .expect("run must stop")
        .unwrap();
    assert_eq!(st.code(), Some(0));
    assert!(env.run(&["list"]).await.1.contains("paused"));
    assert_eq!(env.run(&["resume", "1"]).await.0, 0);
    assert_eq!(env.run(&["rm", "1"]).await.0, 0);
    assert!(
        env.files().is_empty(),
        "rm deletes partial data: {:?}",
        env.files()
    );
}

/// `cawget-cli` with a throwaway XDG data directory (never the user's real one), none of the database
/// variables inherited, and `vars` set.
fn cli_in(xdg: &std::path::Path, vars: &[(&str, &std::path::Path)], args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_cawget-cli"));
    c.args(args)
        .env("XDG_DATA_HOME", xdg)
        .env_remove("CAWGET_DB")
        .env_remove("JACKDAW_DB")
        .env_remove("SEGGET_DB")
        .envs(vars.iter().copied())
        .stdin(Stdio::null());
    c
}

async fn output(mut c: Command) -> String {
    let o = c.output().await.unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(o.status.success(), "{text}");
    text
}

/// Creates a queue at `db` holding one paused job for `url`.
async fn make_queue(xdg: &std::path::Path, db: &std::path::Path, url: &str) {
    let args = ["--db", db.to_str().unwrap(), "add", "--paused", url];
    output(cli_in(xdg, &[], &args)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn default_database_is_migrated_from_each_legacy_location_once() {
    for legacy in ["jackdaw/jackdaw.db", "segget/segget.db"] {
        let xdg = tempfile::tempdir().unwrap();
        let old = xdg.path().join(legacy);
        let new = xdg.path().join("cawget/cawget.db");
        make_queue(xdg.path(), &old, "http://example.invalid/old.bin").await;
        let before = std::fs::read(&old).unwrap();
        assert!(!new.exists());
        let text = output(cli_in(xdg.path(), &[], &["list"])).await;
        assert!(
            text.contains("copied the download queue")
                && text.contains(legacy)
                && text.contains("old.bin"),
            "{legacy}: {text}"
        );
        assert!(new.exists());
        assert_eq!(
            std::fs::read(&old).unwrap(),
            before,
            "{legacy}: the old database must not change"
        );
        let text = output(cli_in(xdg.path(), &[], &["list"])).await;
        assert!(
            !text.contains("copied"),
            "{legacy}: migration happens once: {text}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_newer_legacy_location_wins_over_the_older_one() {
    let xdg = tempfile::tempdir().unwrap();
    let (jackdaw, segget) = (
        xdg.path().join("jackdaw/jackdaw.db"),
        xdg.path().join("segget/segget.db"),
    );
    make_queue(
        xdg.path(),
        &segget,
        "http://example.invalid/from-segget.bin",
    )
    .await;
    make_queue(
        xdg.path(),
        &jackdaw,
        "http://example.invalid/from-jackdaw.bin",
    )
    .await;
    let segget_before = std::fs::read(&segget).unwrap();
    let text = output(cli_in(xdg.path(), &[], &["list"])).await;
    assert!(text.contains("from-jackdaw.bin"), "{text}");
    assert!(!text.contains("from-segget.bin"), "{text}");
    assert_eq!(std::fs::read(&segget).unwrap(), segget_before);
}

#[tokio::test(flavor = "multi_thread")]
async fn deprecated_database_variables_still_work_with_a_note() {
    let xdg = tempfile::tempdir().unwrap();
    let dbs = tempfile::tempdir().unwrap();
    let (c, j, s) = (
        dbs.path().join("c.db"),
        dbs.path().join("j.db"),
        dbs.path().join("s.db"),
    );
    make_queue(xdg.path(), &c, "http://example.invalid/c.bin").await;
    make_queue(xdg.path(), &j, "http://example.invalid/j.bin").await;
    make_queue(xdg.path(), &s, "http://example.invalid/s.bin").await;

    let text = output(cli_in(xdg.path(), &[("SEGGET_DB", &s)], &["list"])).await;
    assert!(
        text.contains("s.bin") && text.contains("SEGGET_DB is deprecated, use CAWGET_DB"),
        "{text}"
    );

    let vars = [("SEGGET_DB", s.as_path()), ("JACKDAW_DB", j.as_path())];
    let text = output(cli_in(xdg.path(), &vars, &["list"])).await;
    assert!(text.contains("j.bin") && !text.contains("s.bin"), "{text}");
    assert!(
        text.contains("JACKDAW_DB is deprecated, use CAWGET_DB"),
        "{text}"
    );

    let vars = [
        ("SEGGET_DB", s.as_path()),
        ("JACKDAW_DB", j.as_path()),
        ("CAWGET_DB", c.as_path()),
    ];
    let text = output(cli_in(xdg.path(), &vars, &["list"])).await;
    assert!(
        text.contains("c.bin") && !text.contains("j.bin") && !text.contains("s.bin"),
        "{text}"
    );
    assert!(!text.contains("deprecated"), "{text}");

    assert!(
        !xdg.path().join("cawget").exists(),
        "a database named by a variable never touches the default location"
    );
}
