//! Secret storage behind a small trait.
//!
//! `cawget-core` never stores credentials in the SQLite database. Queue secrets
//! (request headers, cookies and URLs that carry userinfo) live in the system
//! keyring; the database only holds a reference. The [`SecretStore`] trait
//! abstracts the keyring, with [`KeyringSecretStore`] as the real (Secret
//! Service based) implementation and [`MemorySecretStore`] as an in-memory mock
//! for tests.
//!
//! There is intentionally no plaintext fallback: when no Secret Service is
//! available, [`KeyringSecretStore::available`] returns `false` and every call
//! that would store a secret fails with [`Error::KeyringUnavailable`](crate::Error::KeyringUnavailable).

use crate::error::{Error, Result};
use crate::store::Job;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// The Secret Service collection/service name used for every keyring entry.
pub const KEYRING_SERVICE: &str = crate::product_name!();

/// A reference to a secret in the store (what the database keeps).
pub type SecretRef = String;

/// A Secret store. Implementations must be cheap to share (`Send + Sync`).
pub trait SecretStore: Send + Sync {
    /// Whether the store can currently be used. `false` means the system
    /// keyring is unavailable (no Secret Service daemon, locked out, or a
    /// sandbox without permission).
    fn available(&self) -> bool;

    /// Stores `secret` under `key`, replacing any previous value.
    fn put(&self, key: &str, secret: &str) -> Result<()>;

    /// Reads the secret stored under `key`; `Ok(None)` when there is none.
    fn get(&self, key: &str) -> Result<Option<String>>;

    /// Removes the secret under `key`. Missing entries are not an error.
    fn delete(&self, key: &str) -> Result<()>;
}

/// The real store: the Linux Secret Service (GNOME Keyring, KWallet, …) via
/// the `keyring` crate.
#[derive(Debug, Default)]
pub struct KeyringSecretStore;

impl KeyringSecretStore {
    /// Maps a keyring error onto our error type. Store access problems (no
    /// daemon, locked keyring, sandbox denial) are surfaced as
    /// [`Error::KeyringUnavailable`].
    fn map_error(e: keyring::Error) -> Error {
        use keyring::Error as K;
        match e {
            K::NoDefaultStore | K::NoStorageAccess(_) => Error::KeyringUnavailable(e.to_string()),
            other => Error::Keyring(other.to_string()),
        }
    }

    fn entry(key: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(KEYRING_SERVICE, key).map_err(Self::map_error)
    }
}

impl SecretStore for KeyringSecretStore {
    fn available(&self) -> bool {
        keyring::Entry::store_status().is_ok()
    }

    fn put(&self, key: &str, secret: &str) -> Result<()> {
        if !self.available() {
            return Err(Error::KeyringUnavailable(
                "no Secret Service is available (GNOME Keyring or KWallet is not running, or the app is sandboxed without permission)".into(),
            ));
        }
        Self::entry(key)?
            .set_password(secret)
            .map_err(Self::map_error)
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        match Self::entry(key)?.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(Self::map_error(e)),
        }
    }

    fn delete(&self, key: &str) -> Result<()> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(Self::map_error(e)),
        }
    }
}

/// In-memory mock for tests. Can be made unavailable, or made to fail / corrupt
/// reads, to exercise the error paths.
#[derive(Debug, Default)]
pub struct MemorySecretStore {
    available: AtomicBool,
    /// When true, `get` returns a changed value the first time it is called for
    /// a key (simulates a store that returns different data than it stored).
    corrupt_once: AtomicBool,
    /// When true, `put` fails (simulates a store that cannot write).
    fail_puts: AtomicBool,
    secrets: Mutex<HashMap<String, String>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self {
            available: AtomicBool::new(true),
            corrupt_once: AtomicBool::new(false),
            fail_puts: AtomicBool::new(false),
            secrets: Mutex::new(HashMap::new()),
        }
    }

    /// A store that is present but refuses every operation (like a keyring
    /// that is unavailable).
    pub fn unavailable() -> Self {
        let s = Self::new();
        s.available.store(false, Ordering::Relaxed);
        s
    }

    /// A store whose next `get` of a key returns a different value than stored
    /// (used to test read-back verification in the migration).
    pub fn corrupt_next_read(&self) {
        self.corrupt_once.store(true, Ordering::Relaxed);
    }

    /// Makes `put` fail (used to test what happens when the keyring cannot write).
    pub fn fail_puts(&self) {
        self.fail_puts.store(true, Ordering::Relaxed);
    }

    fn fail_if_unavailable(&self) -> Result<()> {
        if self.available.load(Ordering::Relaxed) {
            Ok(())
        } else {
            Err(Error::KeyringUnavailable(
                "no Secret Service is available (test store)".into(),
            ))
        }
    }
}

impl SecretStore for MemorySecretStore {
    fn available(&self) -> bool {
        self.available.load(Ordering::Relaxed)
    }

    fn put(&self, key: &str, secret: &str) -> Result<()> {
        self.fail_if_unavailable()?;
        if self.fail_puts.load(Ordering::Relaxed) {
            return Err(Error::Keyring("put failed (test)".into()));
        }
        self.secrets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key.to_string(), secret.to_string());
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        self.fail_if_unavailable()?;
        let secrets = self.secrets.lock().unwrap_or_else(|e| e.into_inner());
        let value = secrets.get(key).cloned();
        if self.corrupt_once.swap(false, Ordering::Relaxed) {
            return Ok(value.map(|v| format!("corrupted:{v}")));
        }
        Ok(value)
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.fail_if_unavailable()?;
        self.secrets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(key);
        Ok(())
    }
}

/// Everything secret about one queued job, stored as a JSON blob in the
/// keyring under the job's reference.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobSecret {
    /// The address as entered, including any `user:password@` userinfo.
    pub url: String,
    /// All request headers (`Name: value`), including generated `Authorization`
    /// lines.
    pub headers: Vec<String>,
    /// The raw `Cookie:` value, if any.
    pub cookie: Option<String>,
}

/// The keyring key for a job's secret.
pub fn job_key(id: i64) -> SecretRef {
    format!("job-{id}")
}

/// The keyring key for the proxy credentials.
pub const PROXY_KEY: &str = "proxy";

/// Whether a URL carries userinfo (`user:password@host`).
pub fn url_has_userinfo(url: &str) -> bool {
    reqwest::Url::parse(url)
        .map(|u| !u.username().is_empty())
        .unwrap_or(false)
}

/// Strips userinfo from a URL, keeping the rest of the string exactly as written
/// (no normalization, no added trailing slash). Returns the URL unchanged when it
/// has no userinfo.
pub fn strip_url_userinfo(url: &str) -> Result<String> {
    let u = reqwest::Url::parse(url).map_err(|e| Error::invalid(format!("bad URL {url}: {e}")))?;
    if u.username().is_empty() {
        return Ok(url.to_string());
    }
    let Some(scheme_end) = url.find("://") else {
        return Err(Error::invalid(format!("bad URL {url}")));
    };
    let after_scheme = &url[scheme_end + 3..];
    let Some(at) = after_scheme.find('@') else {
        return Err(Error::invalid(format!("bad URL {url}")));
    };
    Ok(format!(
        "{}{}",
        &url[..scheme_end + 3],
        &after_scheme[at + 1..]
    ))
}

/// Whether a job row still carries plaintext credentials in the database.
/// Used by the migration and by the "no keyring" policy checks.
pub fn job_has_plaintext(url: &str, headers: &[String], cookie: Option<&str>) -> bool {
    url_has_userinfo(url) || !headers.is_empty() || cookie.is_some_and(|c| !c.is_empty())
}

/// Hydrates a `Job` from the secret store, filling the URL, headers and cookie
/// that the database no longer keeps. Jobs without a secret reference are
/// returned unchanged.
pub fn hydrate_job(job: &mut Job, secrets: &dyn SecretStore) -> Result<()> {
    if job.secret_ref.is_empty() {
        return Ok(());
    }
    let Some(json) = secrets.get(&job.secret_ref)? else {
        return Err(Error::Keyring(format!(
            "credential for job {} is missing from the system keyring (was it deleted?)",
            job.id
        )));
    };
    let secret: JobSecret = serde_json::from_str(&json)
        .map_err(|e| Error::Keyring(format!("stored job credential is corrupt: {e}")))?;
    job.url = secret.url;
    job.headers = secret.headers;
    job.cookie = secret.cookie;
    Ok(())
}

// ---- migration of existing plaintext data ----

use crate::store::Store;
use std::path::{Path, PathBuf};

/// Outcome of the one-time plaintext→keyring migration.
#[derive(Debug, Clone)]
pub enum KeyringMigration {
    /// Nothing to migrate (no credentials in the database).
    NotNeeded,
    /// Every plaintext credential found was moved to the keyring and verified.
    Migrated {
        jobs: usize,
        proxy: bool,
        /// Timestamped backup of the pre-migration database (plaintext inside).
        backup: Option<PathBuf>,
    },
    /// The keyring was not available: nothing was changed and the plaintext stays;
    /// the migration is retried on the next start.
    Unavailable { message: String },
    /// The migration could not be completed; already-migrated rows stay migrated and
    /// the rest keeps its plaintext (the next start retries them).
    Failed { error: String },
}

impl KeyringMigration {
    /// A short, human-readable summary (for logs/about dialogs).
    pub fn summary(&self) -> String {
        match self {
            KeyringMigration::NotNeeded => "no credentials to migrate".into(),
            KeyringMigration::Migrated {
                jobs,
                proxy,
                backup,
            } => {
                let mut s = format!(
                    "migrated {jobs} download login{}",
                    if *jobs == 1 { "" } else { "s" }
                );
                if *proxy {
                    s.push_str(" and the proxy login");
                }
                s.push_str(" to the system keyring");
                if let Some(b) = backup {
                    s.push_str(&format!(" (plaintext backup: {})", b.display()));
                }
                s
            }
            KeyringMigration::Unavailable { message } => {
                format!(
                    "system keyring unavailable; logins stay in the database until it is available ({message})"
                )
            }
            KeyringMigration::Failed { error } => {
                format!(
                    "credential migration incomplete; remaining logins stay in the database ({error})"
                )
            }
        }
    }
}

/// Copies the database to a private, timestamped backup next to it.
fn backup_database(db_path: &Path) -> Result<PathBuf> {
    let dir = db_path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .ok_or_else(|| Error::invalid("database path has no directory"))?;
    let name = db_path
        .file_name()
        .ok_or_else(|| Error::invalid("database path has no file name"))?
        .to_string_lossy();
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
    let dest = dir.join(format!("{name}.pre-keyring-{stamp}.bak"));
    let quoted = dest.to_string_lossy().replace('\'', "''");
    let conn = rusqlite::Connection::open(db_path)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch(&format!("VACUUM INTO '{quoted}'"))?;
    drop(conn);
    crate::paths::set_private_file_mode(&dest)?;
    Ok(dest)
}

/// Moves plaintext credentials from the database into the secret store.
///
/// * Scans `jobs` (headers/cookie/URL userinfo) and the `proxy` setting.
/// * If the secret store is unavailable, changes nothing and returns
///   [`KeyringMigration::Unavailable`] (the caller logs and retries next start).
/// * Otherwise backs the database up (a `VACUUM INTO` snapshot next to it), then for
///   each credential writes it to the keyring, reads it back to verify, and only
///   then clears the plaintext column/URL userinfo in one SQLite statement.
/// * Deterministic keyring references make the migration idempotent and safe to
///   interrupt: a retry overwrites the same entry and continues with the rows that
///   still have plaintext.
pub fn migrate_plaintext_secrets(
    db: &Store,
    secrets: &dyn SecretStore,
    db_path: Option<&Path>,
) -> KeyringMigration {
    let jobs = match db.plaintext_jobs() {
        Ok(j) => j,
        Err(e) => {
            return KeyringMigration::Failed {
                error: e.to_string(),
            };
        }
    };
    let proxy = match db.proxy_setting() {
        Ok(Some(p)) if url_has_userinfo(&p) => Some(p),
        Ok(_) => None,
        Err(e) => {
            return KeyringMigration::Failed {
                error: e.to_string(),
            };
        }
    };
    let jobs: Vec<_> = jobs
        .into_iter()
        .filter(|j| job_has_plaintext(&j.url, &j.headers, j.cookie.as_deref()))
        .collect();
    if jobs.is_empty() && proxy.is_none() {
        return KeyringMigration::NotNeeded;
    }
    if !secrets.available() {
        return KeyringMigration::Unavailable {
            message: "no Secret Service is running (GNOME Keyring / KWallet) or the app is sandboxed without permission".into(),
        };
    }
    let backup = match db_path {
        Some(p) => match backup_database(p) {
            Ok(b) => Some(b),
            Err(e) => {
                return KeyringMigration::Failed {
                    error: e.to_string(),
                };
            }
        },
        None => None,
    };

    let mut migrated_jobs = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for j in &jobs {
        let secret = JobSecret {
            url: j.url.clone(),
            headers: j.headers.clone(),
            cookie: j.cookie.clone(),
        };
        let json = match serde_json::to_string(&secret) {
            Ok(j) => j,
            Err(e) => {
                failures.push(e.to_string());
                continue;
            }
        };
        let key = job_key(j.id);
        if let Err(e) = secrets.put(&key, &json) {
            failures.push(format!("storing credentials of job {}: {e}", j.id));
            continue;
        }
        let ok = match secrets.get(&key) {
            Ok(Some(v)) => v == json,
            Ok(None) => false,
            Err(e) => {
                failures.push(format!("verifying credentials of job {}: {e}", j.id));
                continue;
            }
        };
        if !ok {
            failures.push(format!(
                "read-back verification failed for job {}; its plaintext is kept",
                j.id
            ));
            continue;
        }
        let url = match strip_url_userinfo(&j.url) {
            Ok(u) => u,
            Err(e) => {
                failures.push(e.to_string());
                continue;
            }
        };
        if let Err(e) = db.set_credentials(j.id, &url, &[], None, &key) {
            failures.push(e.to_string());
            continue;
        }
        migrated_jobs += 1;
    }

    let mut migrated_proxy = false;
    if let Some(full) = &proxy {
        let mut proxy_failed = false;
        if let Err(e) = secrets.put(PROXY_KEY, full) {
            proxy_failed = true;
            failures.push(format!("storing proxy credentials: {e}"));
        } else {
            match secrets.get(PROXY_KEY) {
                Ok(Some(v)) if v == *full => {}
                Ok(_) => {
                    proxy_failed = true;
                    failures.push(
                        "read-back verification failed for the proxy credentials; they are kept"
                            .into(),
                    );
                }
                Err(e) => {
                    proxy_failed = true;
                    failures.push(e.to_string());
                }
            }
        }
        if !proxy_failed {
            match strip_url_userinfo(full).and_then(|url| {
                db.set_setting("proxy", &url)
                    .and_then(|()| db.set_setting("keyring.proxy_ref", PROXY_KEY))
            }) {
                Ok(()) => migrated_proxy = true,
                Err(e) => failures.push(e.to_string()),
            }
        }
    }

    if !failures.is_empty() {
        return KeyringMigration::Failed {
            error: failures.join("; "),
        };
    }

    KeyringMigration::Migrated {
        jobs: migrated_jobs,
        proxy: migrated_proxy,
        backup,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_roundtrip_and_delete() {
        let s = MemorySecretStore::new();
        assert!(s.available());
        assert_eq!(s.get("k").unwrap(), None);
        s.put("k", "v1").unwrap();
        s.put("k", "v2").unwrap();
        assert_eq!(s.get("k").unwrap().as_deref(), Some("v2"));
        s.delete("k").unwrap();
        assert_eq!(s.get("k").unwrap(), None);
        s.delete("missing").unwrap(); // not an error
    }

    #[test]
    fn unavailable_store_refuses_everything() {
        let s = MemorySecretStore::unavailable();
        assert!(!s.available());
        assert!(matches!(s.put("k", "v"), Err(Error::KeyringUnavailable(_))));
        assert!(matches!(s.get("k"), Err(Error::KeyringUnavailable(_))));
        assert!(matches!(s.delete("k"), Err(Error::KeyringUnavailable(_))));
    }

    #[test]
    fn url_userinfo_detection_and_stripping() {
        assert!(url_has_userinfo("http://user:pass@example.org/f"));
        assert!(url_has_userinfo("https://u@example.org/"));
        assert!(!url_has_userinfo("http://example.org/a@b"));
        assert!(!url_has_userinfo("http://example.org/"));
        assert_eq!(
            strip_url_userinfo("http://user:pass@example.org:8080/f").unwrap(),
            "http://example.org:8080/f"
        );
        assert_eq!(
            strip_url_userinfo("https://example.org/f").unwrap(),
            "https://example.org/f"
        );
        assert!(strip_url_userinfo("not a url").is_err());
    }

    #[test]
    fn job_secret_serialization() {
        let secret = JobSecret {
            url: "http://u:p@h/f".into(),
            headers: vec!["Authorization: Basic eHh4".into()],
            cookie: Some("sid=1".into()),
        };
        let json = serde_json::to_string(&secret).unwrap();
        let back: JobSecret = serde_json::from_str(&json).unwrap();
        assert_eq!(back, secret);
    }

    #[test]
    fn hydrate_job_fills_fields_and_skips_empty_ref() {
        let s = MemorySecretStore::new();
        let mut job = Job {
            id: 1,
            url: "http://h/f".into(),
            out_path: String::new(),
            state: crate::store::State::Queued,
            referer: None,
            user_agent: None,
            headers: vec![],
            cookie: None,
            sha256: None,
            etag: None,
            last_modified: None,
            error: None,
            size: None,
            accept_ranges: false,
            segments: 0,
            created: 0,
            not_before: 0,
            secret_ref: String::new(),
        };
        hydrate_job(&mut job, &s).unwrap();
        assert!(job.headers.is_empty());

        job.secret_ref = job_key(1);
        s.put(
            &job.secret_ref,
            &serde_json::to_string(&JobSecret {
                url: "http://u:p@h/f".into(),
                headers: vec!["X-Test: 1".into()],
                cookie: Some("sid=1".into()),
            })
            .unwrap(),
        )
        .unwrap();
        hydrate_job(&mut job, &s).unwrap();
        assert_eq!(job.url, "http://u:p@h/f");
        assert_eq!(job.headers, vec!["X-Test: 1"]);
        assert_eq!(job.cookie.as_deref(), Some("sid=1"));

        // Missing entry is a clear error, never silent.
        job.secret_ref = job_key(99);
        assert!(matches!(hydrate_job(&mut job, &s), Err(Error::Keyring(_))));
    }
}
