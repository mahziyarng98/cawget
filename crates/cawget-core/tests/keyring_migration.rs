//! Tests for the keyring credential storage and the plaintext→keyring migration.
//! All secrets are dummy values; the Secret Service is never touched (the
//! in-memory mock stands in for it).

use cawget_core::secrets::{
    KeyringMigration, MemorySecretStore, SecretStore, job_key, migrate_plaintext_secrets,
};
use cawget_core::store::NewJob;
use cawget_core::{AddRequest, Error, Manager, Store};
use std::sync::Arc;

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
        serde_json::from_str(&secrets.get(&job_key(id1)).unwrap().unwrap()).unwrap();
    assert_eq!(s1.url, "http://example.org/file.bin");
    assert_eq!(
        s1.headers,
        vec!["Authorization: Basic c2VjcmV0", "X-Api-Key: dummy-key"]
    );
    assert_eq!(s1.cookie.as_deref(), Some("sid=1; secret=abc"));
    let s3: cawget_core::secrets::JobSecret =
        serde_json::from_str(&secrets.get(&job_key(id3)).unwrap().unwrap()).unwrap();
    assert_eq!(s3.url, "http://user:pass@example.org/private");
    // Proxy: setting is sanitized, full URL is in the store.
    assert_eq!(
        store.get_setting("proxy").unwrap().as_deref(),
        Some("http://proxy.example:3128")
    );
    assert_eq!(
        store.get_setting("keyring.proxy_ref").unwrap().as_deref(),
        Some("proxy")
    );
    assert_eq!(
        secrets.get("proxy").unwrap().as_deref(),
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
    secrets.fail_key(&job_key(id1));

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
    // The database keeps the sanitized URL; settings() returns the full one.
    assert_eq!(
        m.store().get_setting("proxy").unwrap().as_deref(),
        Some("http://proxy.example:3128")
    );
    assert_eq!(m.settings().proxy, "http://user:pass@proxy.example:3128");
    assert_eq!(
        secrets.get("proxy").unwrap().as_deref(),
        Some("http://user:pass@proxy.example:3128")
    );

    // A plain proxy clears the saved credentials.
    m.set_setting("proxy", "http://plain.example:3128").unwrap();
    assert_eq!(m.settings().proxy, "http://plain.example:3128");
    assert!(secrets.get("proxy").unwrap().is_none());
    assert_eq!(m.store().get_setting("keyring.proxy_ref").unwrap(), None);

    // "none" and "" reset to system/direct.
    m.set_setting("proxy", "none").unwrap();
    assert_eq!(m.settings().proxy, "none");
    m.set_setting("proxy", "").unwrap();
    assert_eq!(m.settings().proxy, "");
}
