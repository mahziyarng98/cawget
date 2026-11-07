//! Tests against a REAL Secret Service (gnome-keyring-daemon). Ignored by default and
//! skipped unless `CAWGET_PRIVATE_KEYRING_TEST=1`: run them only inside a private,
//! throw-away D-Bus session with a temporary HOME and XDG_RUNTIME_DIR, for example
//!
//! ```text
//! dbus-run-session --config-file=<bus config without service activation> -- \
//!   env HOME=<tmp> XDG_RUNTIME_DIR=<tmp> CAWGET_PRIVATE_KEYRING_TEST=1 \
//!   cargo test -p cawget-core --test real_keyring -- --ignored
//! ```
//!
//! The test starts, freezes, stops and restarts its *own* keyring daemon (by PID); it
//! never talks to a daemon it did not start. Secrets are dummy values.

use cawget_core::Error;
use cawget_core::secrets::{KeyringSecretStore, SecretStore};
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn enabled() -> bool {
    std::env::var("CAWGET_PRIVATE_KEYRING_TEST").as_deref() == Ok("1")
}

fn start_daemon() -> Child {
    let mut c = Command::new("gnome-keyring-daemon")
        .args(["--unlock", "--foreground", "--components=secrets"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("gnome-keyring-daemon");
    let mut stdin = c.stdin.take().unwrap();
    stdin.write_all(b"dummy-login-password").unwrap();
    drop(stdin);
    c
}

fn signal(c: &Child, sig: &str) {
    let ok = Command::new("kill")
        .args([sig, &c.id().to_string()])
        .status()
        .unwrap()
        .success();
    assert!(ok, "kill {sig} {}", c.id());
}

fn eventually(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + limit;
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

#[test]
#[ignore = "needs a private D-Bus session with gnome-keyring; see the module docs"]
fn real_secret_service_appears_hangs_and_restarts_without_a_new_store() {
    if !enabled() {
        eprintln!("skipped: CAWGET_PRIVATE_KEYRING_TEST is not 1");
        return;
    }
    let store = KeyringSecretStore::with_timeout(Duration::from_secs(3));
    let key = "real-test-1";

    // 1. Started before any keyring exists: a clear "unavailable", nothing cached.
    assert!(!store.available());
    assert!(matches!(
        store.put(key, "RealDummy-1"),
        Err(Error::KeyringUnavailable(_))
    ));

    // 2. The daemon appears: the same store works, no restart.
    let mut daemon = start_daemon();
    assert!(
        eventually(Duration::from_secs(10), || store.available()),
        "keyring never became available"
    );
    store.put(key, "RealDummy-1").unwrap();
    assert_eq!(store.get(key).unwrap().as_deref(), Some("RealDummy-1"));

    // 3. The daemon hangs (SIGSTOP): calls give up after the timeout, then fail fast.
    signal(&daemon, "-STOP");
    let t = Instant::now();
    let e = store.get(key).unwrap_err();
    assert!(matches!(e, Error::KeyringUnavailable(_)), "{e}");
    assert!(t.elapsed() < Duration::from_secs(6), "{:?}", t.elapsed());
    let t = Instant::now();
    assert!(store.get(key).is_err());
    assert!(
        t.elapsed() < Duration::from_millis(500),
        "must fail fast while hung"
    );
    signal(&daemon, "-CONT");
    assert!(
        eventually(Duration::from_secs(15), || store
            .get(key)
            .is_ok_and(|v| v.as_deref() == Some("RealDummy-1"))),
        "store did not recover after the daemon continued"
    );

    // 4. The daemon goes away and comes back: the stale connection is replaced.
    signal(&daemon, "-TERM");
    let _ = daemon.wait();
    assert!(
        eventually(Duration::from_secs(10), || matches!(
            store.get(key),
            Err(Error::KeyringUnavailable(_))
        )),
        "a dead daemon must be reported as unavailable"
    );
    daemon = start_daemon();
    assert!(
        eventually(Duration::from_secs(10), || store
            .get(key)
            .is_ok_and(|v| v.as_deref() == Some("RealDummy-1"))),
        "store did not reconnect to the restarted daemon"
    );
    store.delete(key).unwrap();
    assert_eq!(store.get(key).unwrap(), None);
    signal(&daemon, "-TERM");
    let _ = daemon.wait();
}
