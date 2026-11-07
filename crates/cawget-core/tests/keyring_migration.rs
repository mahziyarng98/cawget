//! Tests for the keyring credential storage and the plaintext→keyring migration.
//! All secrets are dummy values; the Secret Service is never touched (the
//! in-memory mock stands in for it).

use cawget_core::secrets::{
    KeyringMigration, MemorySecretStore, RECONCILE_MIN_AGE, SecretStore, job_key,
    migrate_plaintext_secrets, reconcile_secrets,
};
use cawget_core::store::NewJob;
use cawget_core::{AddRequest, Error, Manager, Store};
use std::sync::Arc;

/// The keyring key of job `id` in the database at `db`.
fn jk(db: &std::path::Path, id: i64) -> String {
    job_key(&Store::open(db).unwrap().keyring_namespace().unwrap(), id)
}

/// The keyring key of the proxy login of the database at `db`.
fn pk(db: &std::path::Path) -> String {
    cawget_core::secrets::proxy_key(&Store::open(db).unwrap().keyring_namespace().unwrap())
}

fn temp_db() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("cawget.db");
    (tmp, db)
}

/// A store that fails individual `get` calls (read-back verification failures).
#[derive(Debug)]
struct FailGets {
    inner: MemorySecretStore,
    fail_keys: std::sync::Mutex<std::collections::HashSet<String>>,
}

impl FailGets {
    fn new() -> Self {
        Self {
            inner: MemorySecretStore::new(),
            fail_keys: Default::default(),
        }
    }
    fn fail_key(&self, key: &str) {
        self.fail_keys.lock().unwrap().insert(key.to_string());
    }
}

impl SecretStore for FailGets {
    fn available(&self) -> bool {
        self.inner.available()
    }
    fn put(&self, key: &str, secret: &str) -> cawget_core::Result<()> {
        self.inner.put(key, secret)
    }
    fn get(&self, key: &str) -> cawget_core::Result<Option<String>> {
        if self.fail_keys.lock().unwrap().contains(key) {
            return Err(cawget_core::Error::Keyring("get failed (test)".into()));
        }
        self.inner.get(key)
    }
    fn delete(&self, key: &str) -> cawget_core::Result<()> {
        self.inner.delete(key)
    }
}

fn add_plaintext_job(store: &Store, headers: &[&str], cookie: Option<&str>) -> i64 {
    store
        .add_job(
            &NewJob {
                url: "http://example.org/file.bin".into(),
                headers: headers.iter().map(|s| s.to_string()).collect(),
                cookie: cookie.map(str::to_string),
                ..Default::default()
            },
            1,
        )
        .unwrap()
}

#[test]
fn migration_moves_job_and_proxy_credentials_to_the_secret_store() {
    let (_tmp, db) = temp_db();
    let store = Store::open(&db).unwrap();
    let id1 = add_plaintext_job(
        &store,
        &["Authorization: Basic c2VjcmV0", "X-Api-Key: dummy-key"],
        Some("sid=1; secret=abc"),
    );
    let _id2 = add_plaintext_job(&store, &["Cookie: sid=2"], None);
    // A URL carrying userinfo.
    let id3 = store
        .add_job(
            &NewJob {
                url: "http://user:pass@example.org/private".into(),
                ..Default::default()
            },
            1,
        )
        .unwrap();
    store
        .set_setting("proxy", "http://proxyuser:proxypass@proxy.example:3128")
        .unwrap();
    let secrets = MemorySecretStore::new();

    let outcome = migrate_plaintext_secrets(&store, &secrets, Some(&db));
    match &outcome {
        KeyringMigration::Migrated {
            jobs,
            proxy,
            backup,
        } => {
            assert_eq!(*jobs, 3);
            assert!(*proxy);
            let backup = backup.as_ref().expect("backup must exist");
            assert!(
                backup.is_file(),
                "backup file missing: {}",
                backup.display()
            );
            assert!(
                backup
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("cawget.db.pre-keyring-")
            );
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(backup).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "plaintext backup must be private");
        }
        other => panic!("expected Migrated, got {other:?}"),
    }

    // Plaintext is gone from the database.
    let jobs = store.list_jobs(None).unwrap();
    assert_eq!(jobs.len(), 3);
    for j in &jobs {
        assert!(
            j.headers.is_empty(),
            "headers must be cleared: {:?}",
            j.headers
        );
        assert!(j.cookie.is_none(), "cookie must be cleared");
        assert!(!j.url.contains('@'), "URL must have no userinfo: {}", j.url);
        assert!(!j.secret_ref.is_empty());
    }
    // The secrets are readable from the store.
    let s1: cawget_core::secrets::JobSecret =
        serde_json::from_str(&secrets.get(&jk(&db, id1)).unwrap().unwrap()).unwrap();
    assert_eq!(s1.url, "http://example.org/file.bin");
    assert_eq!(
        s1.headers,
        vec!["Authorization: Basic c2VjcmV0", "X-Api-Key: dummy-key"]
    );
    assert_eq!(s1.cookie.as_deref(), Some("sid=1; secret=abc"));
    let s3: cawget_core::secrets::JobSecret =
        serde_json::from_str(&secrets.get(&jk(&db, id3)).unwrap().unwrap()).unwrap();
    assert_eq!(s3.url, "http://user:pass@example.org/private");
    // Proxy: setting is sanitized, full URL is in the store.
    assert_eq!(
        store.get_setting("proxy").unwrap().as_deref(),
        Some("http://proxy.example:3128")
    );
    assert_eq!(
        store.get_setting("keyring.proxy_ref").unwrap(),
        Some(pk(&db))
    );
    assert_eq!(
        secrets.get(&pk(&db)).unwrap().as_deref(),
        Some("http://proxyuser:proxypass@proxy.example:3128")
    );

    // Second run is a no-op and does not make another backup.
    let backup_files: Vec<_> = std::fs::read_dir(db.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .filter(|n| n.to_string_lossy().contains(".pre-keyring-"))
        .collect();
    let again = migrate_plaintext_secrets(&store, &secrets, Some(&db));
    assert!(matches!(again, KeyringMigration::NotNeeded), "{again:?}");
    let backup_files_after: Vec<_> = std::fs::read_dir(db.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .filter(|n| n.to_string_lossy().contains(".pre-keyring-"))
        .collect();
    assert_eq!(backup_files.len(), backup_files_after.len());
}

#[test]
fn migration_without_a_keyring_leaves_everything_intact() {
    let (_tmp, db) = temp_db();
    let store = Store::open(&db).unwrap();
    add_plaintext_job(&store, &["Authorization: Basic c2VjcmV0"], None);
    store
        .set_setting("proxy", "http://user:pass@proxy.example:3128")
        .unwrap();
    let secrets = MemorySecretStore::unavailable();

    let outcome = migrate_plaintext_secrets(&store, &secrets, Some(&db));
    assert!(
        matches!(outcome, KeyringMigration::Unavailable { .. }),
        "{outcome:?}"
    );

    // Nothing changed: the plaintext is still in the database and no backup exists.
    let jobs = store.list_jobs(None).unwrap();
    assert_eq!(jobs[0].headers, vec!["Authorization: Basic c2VjcmV0"]);
    assert_eq!(
        store.get_setting("proxy").unwrap().as_deref(),
        Some("http://user:pass@proxy.example:3128")
    );
    assert_eq!(store.get_setting("keyring.proxy_ref").unwrap(), None);
    assert!(std::fs::read_dir(db.parent().unwrap()).unwrap().all(|e| {
        !e.unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".pre-keyring-")
    }));

    // Once the keyring appears, the same migration succeeds.
    let secrets = MemorySecretStore::new();
    let outcome = migrate_plaintext_secrets(&store, &secrets, Some(&db));
    assert!(
        matches!(
            outcome,
            KeyringMigration::Migrated {
                jobs: 1,
                proxy: true,
                ..
            }
        ),
        "{outcome:?}"
    );
}

#[test]
fn migration_with_a_failed_readback_keeps_that_row_plaintext() {
    let (_tmp, db) = temp_db();
    let store = Store::open(&db).unwrap();
    let id1 = add_plaintext_job(&store, &["Authorization: Basic c2VjcmV0"], None);
    let id2 = add_plaintext_job(&store, &["X-Test: ok"], None);
    let secrets = FailGets::new();
    secrets.fail_key(&jk(&db, id1));

    let outcome = migrate_plaintext_secrets(&store, &secrets, Some(&db));
    assert!(
        matches!(outcome, KeyringMigration::Failed { .. }),
        "{outcome:?}"
    );

    // The failed job kept its plaintext and has no reference.
    let j1 = store.require_job(id1).unwrap();
    assert_eq!(j1.headers, vec!["Authorization: Basic c2VjcmV0"]);
    assert!(j1.secret_ref.is_empty());
    // The other job was migrated.
    let j2 = store.require_job(id2).unwrap();
    assert!(j2.headers.is_empty());
    assert!(!j2.secret_ref.is_empty());
}

#[test]
fn manager_refuses_credentials_without_a_keyring() {
    let (_tmp, db) = temp_db();
    let store = Store::open(&db).unwrap();
    let secrets = Arc::new(MemorySecretStore::unavailable());
    let m = Manager::with_store_and_secrets(store, secrets);

    let err = m
        .add(AddRequest {
            url: "http://example.org/f".into(),
            headers: vec!["Authorization: Basic c2VjcmV0".into()],
            ..Default::default()
        })
        .unwrap_err();
    assert!(matches!(err, Error::KeyringUnavailable(_)), "{err:?}");
    assert!(m.list().unwrap().is_empty(), "nothing may be queued");

    let err = m
        .set_setting("proxy", "http://user:pass@proxy.example:3128")
        .unwrap_err();
    assert!(matches!(err, Error::KeyringUnavailable(_)), "{err:?}");
    assert_eq!(m.settings().proxy, "");
}

#[test]
fn manager_does_not_queue_a_job_when_the_secret_store_cannot_write() {
    let (_tmp, db) = temp_db();
    let store = Store::open(&db).unwrap();
    let secrets = Arc::new(MemorySecretStore::new());
    secrets.fail_puts();
    let m = Manager::with_store_and_secrets(store, secrets);

    let err = m
        .add(AddRequest {
            url: "http://example.org/f".into(),
            headers: vec!["Authorization: Basic c2VjcmV0".into()],
            ..Default::default()
        })
        .unwrap_err();
    assert!(matches!(err, Error::Keyring(_)), "{err:?}");
    assert!(m.list().unwrap().is_empty(), "no row may be left behind");
}

#[test]
fn manager_stores_secrets_in_the_store_and_hydrates_them() {
    let (_tmp, db) = temp_db();
    let store = Store::open(&db).unwrap();
    let secrets = Arc::new(MemorySecretStore::new());
    let m = Manager::with_store_and_secrets(store, secrets.clone());

    let id = m
        .add(AddRequest {
            url: "http://user:pass@example.org/f".into(),
            headers: vec!["X-Test: abc".into()],
            cookie: Some("sid=9".into()),
            ..Default::default()
        })
        .unwrap();

    // The database row has no credentials.
    let job = m.store().require_job(id).unwrap();
    assert!(job.headers.is_empty());
    assert!(job.cookie.is_none());
    assert!(!job.url.contains('@'));
    assert!(!job.secret_ref.is_empty());

    // The secret is in the store, and hydration restores everything.
    let secret: cawget_core::secrets::JobSecret =
        serde_json::from_str(&secrets.get(&job.secret_ref).unwrap().unwrap()).unwrap();
    assert_eq!(secret.url, "http://user:pass@example.org/f");
    assert_eq!(secret.headers, vec!["X-Test: abc"]);
    assert_eq!(secret.cookie.as_deref(), Some("sid=9"));
    let mut hydrated = m.store().require_job(id).unwrap();
    cawget_core::secrets::hydrate_job(&mut hydrated, secrets.as_ref()).unwrap();
    assert_eq!(hydrated.url, "http://user:pass@example.org/f");
    assert_eq!(hydrated.headers, vec!["X-Test: abc"]);
    assert_eq!(hydrated.cookie.as_deref(), Some("sid=9"));

    // Removing the job removes its secret.
    m.remove(id, false).unwrap();
    assert!(secrets.get(&job.secret_ref).unwrap().is_none());
}

#[test]
fn manager_proxy_setting_roundtrip() {
    let (_tmp, db) = temp_db();
    let store = Store::open(&db).unwrap();
    let secrets = Arc::new(MemorySecretStore::new());
    let m = Manager::with_store_and_secrets(store, secrets.clone());

    m.set_setting("proxy", "http://user:pass@proxy.example:3128")
        .unwrap();
    // The database keeps the sanitized URL; settings() shows the login only masked.
    assert_eq!(
        m.store().get_setting("proxy").unwrap().as_deref(),
        Some("http://proxy.example:3128")
    );
    assert_eq!(m.settings().proxy, "http://***@proxy.example:3128");
    assert!(!format!("{:?}", m.settings()).contains("pass"));
    // Giving the masked form back (an unchanged settings dialog) keeps the login.
    m.set_setting("proxy", "http://***@proxy.example:3128")
        .unwrap();
    assert_eq!(
        secrets.get(&pk(&db)).unwrap().as_deref(),
        Some("http://user:pass@proxy.example:3128")
    );
    // A masked form for another host is refused (the password must be entered again).
    assert!(
        m.set_setting("proxy", "http://***@other.example:3128")
            .is_err()
    );
    assert_eq!(
        secrets.get(&pk(&db)).unwrap().as_deref(),
        Some("http://user:pass@proxy.example:3128")
    );

    // A plain proxy clears the saved credentials.
    m.set_setting("proxy", "http://plain.example:3128").unwrap();
    assert_eq!(m.settings().proxy, "http://plain.example:3128");
    assert!(secrets.get(&pk(&db)).unwrap().is_none());
    assert_eq!(m.store().get_setting("keyring.proxy_ref").unwrap(), None);

    // "none" and "" reset to system/direct.
    m.set_setting("proxy", "none").unwrap();
    assert_eq!(m.settings().proxy, "none");
    m.set_setting("proxy", "").unwrap();
    assert_eq!(m.settings().proxy, "");
}

#[test]
fn job_with_credentials_is_not_runnable_before_its_secret_is_stored() {
    let (_tmp, db) = temp_db();
    let store = Store::open(&db).unwrap();
    let secrets = Arc::new(MemorySecretStore::new());
    let m = Manager::with_store_and_secrets(store, secrets.clone());

    // Queued add: ends up queued, with the reference set and the secret stored.
    let id = m
        .add(AddRequest {
            url: "http://example.org/f".into(),
            headers: vec!["X-Test: dummy".into()],
            ..Default::default()
        })
        .unwrap();
    let job = m.store().require_job(id).unwrap();
    assert_eq!(job.state, cawget_core::State::Queued);
    assert_eq!(job.secret_ref, jk(&db, id));
    assert!(secrets.get(&job.secret_ref).unwrap().is_some());

    // Paused add stays paused.
    let id2 = m
        .add(AddRequest {
            url: "http://example.org/g".into(),
            headers: vec!["X-Test: dummy".into()],
            paused: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        m.store().require_job(id2).unwrap().state,
        cawget_core::State::Paused
    );
}

#[test]
fn failed_add_leaves_no_keyring_entry_behind() {
    let (_tmp, db) = temp_db();
    let store = Store::open(&db).unwrap();
    let secrets = Arc::new(MemorySecretStore::new());
    let m = Manager::with_store_and_secrets(store, secrets.clone());
    secrets.corrupt_next_read(); // read-back verification fails after the put
    let err = m
        .add(AddRequest {
            url: "http://example.org/f".into(),
            headers: vec!["X-Test: dummy".into()],
            ..Default::default()
        })
        .unwrap_err();
    assert!(matches!(err, Error::Keyring(_)), "{err:?}");
    assert!(m.list().unwrap().is_empty());
    assert!(
        secrets.get(&jk(&db, 1)).unwrap().is_none(),
        "orphaned entry"
    );
}

#[test]
fn migration_leaves_no_plaintext_in_the_database_file_or_wal() {
    let (_tmp, db) = temp_db();
    let store = Store::open(&db).unwrap();
    let id = store
        .add_job(
            &NewJob {
                url: "http://scrubuser:ScrubPw-DUMMY@example.org/private".into(),
                headers: vec!["X-Api-Key: ScrubKey-DUMMY".into()],
                cookie: Some("sid=ScrubCookie-DUMMY".into()),
                ..Default::default()
            },
            1,
        )
        .unwrap();
    let _ = id;
    let secrets = MemorySecretStore::new();
    let outcome = migrate_plaintext_secrets(&store, &secrets, Some(&db));
    assert!(
        matches!(outcome, KeyringMigration::Migrated { .. }),
        "{outcome:?}"
    );
    for ext in ["", "-wal"] {
        let mut p = db.as_os_str().to_owned();
        p.push(ext);
        let bytes = std::fs::read(std::path::PathBuf::from(p)).unwrap_or_default();
        for needle in ["ScrubPw-DUMMY", "ScrubKey-DUMMY", "ScrubCookie-DUMMY"] {
            assert!(
                !bytes.windows(needle.len()).any(|w| w == needle.as_bytes()),
                "{needle} still present in db{ext}"
            );
        }
    }
}

fn backups_in(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().contains(".pre-keyring-"))
        .collect()
}

#[test]
fn repeated_failed_migrations_reuse_one_private_backup() {
    use std::os::unix::fs::PermissionsExt;
    let (tmp, db) = temp_db();
    let store = Store::open(&db).unwrap();
    let id1 = add_plaintext_job(&store, &["Authorization: Basic c2VjcmV0"], None);
    let _id2 = add_plaintext_job(&store, &["X-Test: ok"], None);
    let secrets = FailGets::new();
    secrets.fail_key(&jk(&db, id1));

    for _ in 0..3 {
        let outcome = migrate_plaintext_secrets(&store, &secrets, Some(&db));
        assert!(
            matches!(outcome, KeyringMigration::Failed { .. }),
            "{outcome:?}"
        );
    }
    let backups = backups_in(tmp.path());
    assert_eq!(backups.len(), 1, "one backup for all retries: {backups:?}");
    let mode = std::fs::metadata(&backups[0]).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);

    // The keyring works again: the retry completes with the same backup, which still
    // holds the plaintext of every row (also the one that failed first).
    secrets.fail_keys.lock().unwrap().clear();
    match migrate_plaintext_secrets(&store, &secrets, Some(&db)) {
        KeyringMigration::Migrated { backup, .. } => assert_eq!(backup.as_ref(), Some(&backups[0])),
        other => panic!("{other:?}"),
    }
    assert_eq!(backups_in(tmp.path()).len(), 1);
    let bytes = std::fs::read(&backups[0]).unwrap();
    assert!(bytes.windows(14).any(|w| w == b"Basic c2VjcmV0"));
    assert_eq!(
        store
            .get_setting(cawget_core::secrets::BACKUP_SETTING)
            .unwrap(),
        None
    );
}

#[test]
fn manager_works_once_the_keyring_appears_without_a_restart() {
    let (_tmp, db) = temp_db();
    let secrets = Arc::new(MemorySecretStore::unavailable());
    let m = Manager::with_store_and_secrets(Store::open(&db).unwrap(), secrets.clone());
    let req = || AddRequest {
        url: "http://example.org/f".into(),
        headers: vec!["X-Test: dummy".into()],
        ..Default::default()
    };
    assert!(matches!(m.add(req()), Err(Error::KeyringUnavailable(_))));
    secrets.set_available(true); // the daemon started later
    let id = m.add(req()).unwrap();
    assert!(secrets.contains(&jk(&db, id)));
}

#[test]
fn error_texts_never_contain_passwords() {
    let (_tmp, db) = temp_db();
    let m = Manager::with_store_and_secrets(
        Store::open(&db).unwrap(),
        Arc::new(MemorySecretStore::new()),
    );
    for bad in [
        "http://alice:ErrPw-DUMMY@exa mple.org/f",
        "http://alice:ErrPw-DUMMY@example.org:99999/f",
    ] {
        let e = m
            .add(AddRequest {
                url: bad.into(),
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(!e.contains("ErrPw"), "{e}");
    }
    let e = m
        .set_setting("proxy", "http://p:ErrProxy-DUMMY@px:99999")
        .unwrap_err()
        .to_string();
    assert!(!e.contains("ErrProxy"), "{e}");
}

#[test]
fn clearing_the_proxy_while_the_keyring_is_down_is_repaired_later() {
    let (_tmp, db) = temp_db();
    let secrets = Arc::new(MemorySecretStore::new());
    let m = Manager::with_store_and_secrets(Store::open(&db).unwrap(), secrets.clone());
    m.set_setting("proxy", "http://u:ProxyPw-DUMMY@px:3128")
        .unwrap();
    secrets.set_available(false);
    m.set_setting("proxy", "none").unwrap(); // must not fail because of the keyring
    assert_eq!(m.store().get_setting("keyring.proxy_ref").unwrap(), None);
    assert!(
        secrets.contains(&pk(&db)),
        "keyring was down, entry still there"
    );
    let pending = m.store().pending_secrets().unwrap();
    assert_eq!(pending.len(), 1);

    // Keyring back: reconciliation (old enough marks only) removes the orphan.
    secrets.set_available(true);
    let now = pending[0].1;
    assert_eq!(
        reconcile_secrets(m.store(), secrets.as_ref(), now, RECONCILE_MIN_AGE),
        0
    );
    assert!(secrets.contains(&pk(&db)), "young marks are left alone");
    assert_eq!(
        reconcile_secrets(
            m.store(),
            secrets.as_ref(),
            now + RECONCILE_MIN_AGE,
            RECONCILE_MIN_AGE
        ),
        1
    );
    assert!(!secrets.contains(&pk(&db)));
    assert!(m.store().pending_secrets().unwrap().is_empty());
}

#[test]
fn removing_jobs_while_the_keyring_is_down_is_repaired_later() {
    let (_tmp, db) = temp_db();
    let secrets = Arc::new(MemorySecretStore::new());
    let m = Manager::with_store_and_secrets(Store::open(&db).unwrap(), secrets.clone());
    let add = |paused| {
        m.add(AddRequest {
            url: "http://example.org/f".into(),
            cookie: Some("sid=dummy".into()),
            paused,
            ..Default::default()
        })
        .unwrap()
    };
    let (gone, kept) = (add(true), add(true));
    secrets.set_available(false);
    m.remove(gone, false).unwrap();
    secrets.set_available(true);
    let far = cawget_core::util::unix_now() + 3600;
    assert_eq!(reconcile_secrets(m.store(), secrets.as_ref(), far, 0), 1);
    assert!(!secrets.contains(&jk(&db, gone)));
    assert!(secrets.contains(&jk(&db, kept)), "referenced entries stay");
}

#[test]
fn crash_between_db_delete_and_keyring_delete_is_repaired_at_startup() {
    let (_tmp, db) = temp_db();
    let secrets = Arc::new(MemorySecretStore::new());
    let id = {
        let m = Manager::with_store_and_secrets(Store::open(&db).unwrap(), secrets.clone());
        m.add(AddRequest {
            url: "http://u:CrashPw-DUMMY@example.org/f".into(),
            paused: true,
            ..Default::default()
        })
        .unwrap()
    };
    // Simulate the crash: the row and its pending mark are written, the keyring delete
    // never happens.
    let store = Store::open(&db).unwrap();
    store
        .delete_job_marking_secret(id, &jk(&db, id), 0)
        .unwrap();
    assert!(secrets.contains(&jk(&db, id)));
    // Next start.
    assert_eq!(
        reconcile_secrets(
            &store,
            secrets.as_ref(),
            RECONCILE_MIN_AGE,
            RECONCILE_MIN_AGE
        ),
        1
    );
    assert!(!secrets.contains(&jk(&db, id)));
}

#[test]
fn crash_before_the_secret_is_stored_never_downloads_without_login() {
    let (_tmp, db) = temp_db();
    let store = Store::open(&db).unwrap();
    let ns = store.keyring_namespace().unwrap();
    // The row is written with its reference and pending mark, then the process dies.
    let (id, key) = store
        .add_job_with_secret(
            &NewJob {
                url: "http://example.org/f".into(),
                paused: true,
                ..Default::default()
            },
            0,
            |id| job_key(&ns, id),
        )
        .unwrap();
    let secrets = MemorySecretStore::new();
    let mut job = store.require_job(id).unwrap();
    assert_eq!(job.secret_ref, key);
    let e = cawget_core::secrets::hydrate_job(&mut job, &secrets).unwrap_err();
    assert!(
        e.to_string().contains("missing from the system keyring"),
        "{e}"
    );
    // Reconciliation keeps the mark's entry decision to the reference: still referenced.
    assert_eq!(reconcile_secrets(&store, &secrets, 100, 0), 0);
    assert!(store.pending_secrets().unwrap().is_empty());
}

#[test]
fn two_databases_never_share_keyring_entries() {
    let (_t1, db1) = temp_db();
    let (_t2, db2) = temp_db();
    let secrets = Arc::new(MemorySecretStore::new());
    let m1 = Manager::with_store_and_secrets(Store::open(&db1).unwrap(), secrets.clone());
    let m2 = Manager::with_store_and_secrets(Store::open(&db2).unwrap(), secrets.clone());
    let add = |m: &Manager, v: &str| {
        m.add(AddRequest {
            url: "http://example.org/f".into(),
            headers: vec![format!("X-Test: {v}")],
            paused: true,
            ..Default::default()
        })
        .unwrap()
    };
    let (a, b) = (add(&m1, "one"), add(&m2, "two"));
    assert_eq!(a, b, "same row id in both databases");
    assert_ne!(jk(&db1, a), jk(&db2, b));
    // Removing (and reconciling) in one database leaves the other's entry alone.
    m1.remove(a, false).unwrap();
    let far = cawget_core::util::unix_now() + 3600;
    reconcile_secrets(m1.store(), secrets.as_ref(), far, 0);
    let mut j = m2.store().require_job(b).unwrap();
    cawget_core::secrets::hydrate_job(&mut j, secrets.as_ref()).unwrap();
    assert_eq!(j.headers, vec!["X-Test: two"]);
}

#[test]
fn rows_with_keys_from_earlier_versions_still_work() {
    let (_tmp, db) = temp_db();
    let store = Store::open(&db).unwrap();
    let id = add_plaintext_job(&store, &[], None);
    // PR #10 wrote un-namespaced keys ("job-<id>"); the row keeps its exact key.
    store
        .set_credentials(id, "http://example.org/file.bin", &[], None, "job-1")
        .unwrap();
    let secrets = Arc::new(MemorySecretStore::new());
    secrets
        .put(
            "job-1",
            r#"{"url":"http://example.org/file.bin","headers":["X-Old: 1"],"cookie":null}"#,
        )
        .unwrap();
    let m = Manager::with_store_and_secrets(store, secrets.clone());
    let mut j = m.store().require_job(id).unwrap();
    cawget_core::secrets::hydrate_job(&mut j, secrets.as_ref()).unwrap();
    assert_eq!(j.headers, vec!["X-Old: 1"]);
    m.remove(id, false).unwrap();
    assert!(!secrets.contains("job-1"));
}

#[test]
fn a_new_proxy_login_removes_one_saved_under_an_older_key() {
    let (_tmp, db) = temp_db();
    let store = Store::open(&db).unwrap();
    let secrets = Arc::new(MemorySecretStore::new());
    // As written by PR #10: un-namespaced key "proxy".
    secrets
        .put("proxy", "http://u:OldPw-DUMMY@old:3128")
        .unwrap();
    store.set_setting("proxy", "http://old:3128").unwrap();
    store.set_setting("keyring.proxy_ref", "proxy").unwrap();
    let m = Manager::with_store_and_secrets(store, secrets.clone());
    m.set_setting("proxy", "http://u:NewPw-DUMMY@new:3128")
        .unwrap();
    assert!(!secrets.contains("proxy"), "old entry must go");
    assert_eq!(
        secrets.get(&pk(&db)).unwrap().as_deref(),
        Some("http://u:NewPw-DUMMY@new:3128")
    );
}
