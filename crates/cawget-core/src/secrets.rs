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
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

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

/// How long one keyring call may take before it is abandoned. A locked keyring waiting
/// for an unlock prompt, or a D-Bus activation that never completes, would otherwise
/// block the caller for minutes.
pub const KEYRING_TIMEOUT: Duration = Duration::from_secs(30);

const NO_SECRET_SERVICE: &str = "no Secret Service is available (GNOME Keyring or KWallet is not running, or the app is sandboxed without permission)";

type SsStore = zbus_secret_service_keyring_store::Store;
type StoreSlot = Arc<Mutex<Option<Arc<SsStore>>>>;

/// The real store: the Linux Secret Service (GNOME Keyring, KWallet, …) via
/// `keyring-core` and its zbus Secret Service backend.
///
/// * The connection is opened lazily and only a *working* connection is cached: when
///   no Secret Service is running, every call tries again, so credentials work as soon
///   as the keyring appears (no restart needed). A cached connection that went stale
///   (daemon restarted or gone) is dropped and reopened once.
/// * Every call runs on a helper thread and is abandoned after a timeout with a clear
///   [`Error::KeyringUnavailable`]. While an abandoned call is still hanging, further
///   calls fail at once instead of piling up; they work again once it returns.
pub struct KeyringSecretStore {
    store: StoreSlot,
    timeout: Duration,
    /// Calls that timed out and whose helper thread has not returned yet.
    stuck: Arc<AtomicUsize>,
}

impl std::fmt::Debug for KeyringSecretStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyringSecretStore")
            .field("timeout", &self.timeout)
            .field("stuck", &self.stuck.load(Ordering::SeqCst))
            .finish_non_exhaustive()
    }
}

impl Default for KeyringSecretStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Runs `f` on a helper thread and waits at most `timeout` for it. A call that times out
/// is counted in `stuck` until its thread returns; while any call is stuck, new calls fail
/// at once.
pub(crate) fn run_with_timeout<T: Send + 'static>(
    timeout: Duration,
    stuck: &Arc<AtomicUsize>,
    f: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    if stuck.load(Ordering::SeqCst) > 0 {
        return Err(Error::KeyringUnavailable(
            "the system keyring is not responding (an earlier request is still waiting, for example for an unlock prompt)".into(),
        ));
    }
    let (tx, rx) = mpsc::sync_channel::<Result<T>>(1);
    let abandoned = Arc::new(Mutex::new(false));
    let (abandoned2, stuck2) = (Arc::clone(&abandoned), Arc::clone(stuck));
    std::thread::Builder::new()
        .name("cawget-keyring".into())
        .spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
                .unwrap_or_else(|_| Err(Error::Keyring("keyring call panicked".into())));
            let gone = abandoned2.lock().unwrap_or_else(|e| e.into_inner());
            if *gone {
                stuck2.fetch_sub(1, Ordering::SeqCst);
            } else {
                let _ = tx.send(r);
            }
        })
        .map_err(|e| Error::Keyring(format!("cannot start the keyring thread: {e}")))?;
    match rx.recv_timeout(timeout) {
        Ok(r) => r,
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(Error::Keyring("keyring call ended without a result".into()))
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            // Holding the flag's lock, the helper cannot be between "finished" and "sent".
            let mut gone = abandoned.lock().unwrap_or_else(|e| e.into_inner());
            if let Ok(r) = rx.try_recv() {
                return r;
            }
            *gone = true;
            stuck.fetch_add(1, Ordering::SeqCst);
            Err(Error::KeyringUnavailable(format!(
                "the system keyring did not answer within {} s (is an unlock prompt waiting?)",
                timeout.as_secs()
            )))
        }
    }
}

impl KeyringSecretStore {
    pub fn new() -> Self {
        Self::with_timeout(KEYRING_TIMEOUT)
    }

    /// A store whose calls are abandoned after `timeout`.
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            store: Arc::new(Mutex::new(None)),
            timeout,
            stuck: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Maps a keyring error onto our error type. Store access problems (no
    /// daemon, locked keyring, sandbox denial) are surfaced as
    /// [`Error::KeyringUnavailable`].
    fn map_error(e: keyring_core::Error) -> Error {
        use keyring_core::Error as K;
        match e {
            K::NoDefaultStore | K::NoStorageAccess(_) | K::PlatformFailure(_) => {
                Error::KeyringUnavailable(e.to_string())
            }
            other => Error::Keyring(other.to_string()),
        }
    }

    /// The cached connection, or a new one. Failures are never cached.
    fn connect(slot: &StoreSlot, fresh: bool) -> Result<(Arc<SsStore>, bool)> {
        if !fresh && let Some(s) = slot.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            return Ok((Arc::clone(s), true));
        }
        // Connect without holding the lock: this may block (it is bounded by the caller's timeout).
        let s = SsStore::new().map_err(|e| match e {
            keyring_core::Error::PlatformFailure(_) | keyring_core::Error::NoStorageAccess(_) => {
                Error::KeyringUnavailable(format!("{NO_SECRET_SERVICE}: {e}"))
            }
            other => Self::map_error(other),
        })?;
        *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::clone(&s));
        Ok((s, false))
    }

    /// Runs `op` on the entry for `key`, reconnecting once when a cached connection
    /// turned out to be stale.
    fn with_entry<T>(
        slot: &StoreSlot,
        key: &str,
        op: impl Fn(&keyring_core::Entry) -> keyring_core::Result<T>,
    ) -> Result<T> {
        use keyring_core::api::CredentialStoreApi;
        let run = |s: &SsStore| s.build(KEYRING_SERVICE, key, None).and_then(|e| op(&e));
        let (store, cached) = Self::connect(slot, false)?;
        match run(&store) {
            Ok(v) => Ok(v),
            Err(keyring_core::Error::PlatformFailure(_)) if cached => {
                let (store, _) = Self::connect(slot, true)?;
                run(&store).map_err(|e| {
                    if matches!(e, keyring_core::Error::PlatformFailure(_)) {
                        *slot.lock().unwrap_or_else(|e| e.into_inner()) = None;
                    }
                    Self::map_error(e)
                })
            }
            Err(e) => {
                if matches!(e, keyring_core::Error::PlatformFailure(_)) {
                    *slot.lock().unwrap_or_else(|e| e.into_inner()) = None;
                }
                Err(Self::map_error(e))
            }
        }
    }

    fn guarded<T: Send + 'static>(
        &self,
        f: impl FnOnce(&StoreSlot) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let slot = Arc::clone(&self.store);
        run_with_timeout(self.timeout, &self.stuck, move || f(&slot))
    }
}

impl SecretStore for KeyringSecretStore {
    fn available(&self) -> bool {
        self.guarded(|slot| Self::connect(slot, false).map(|_| ()))
            .is_ok()
    }

    fn put(&self, key: &str, secret: &str) -> Result<()> {
        let (key, secret) = (key.to_string(), secret.to_string());
        self.guarded(move |slot| Self::with_entry(slot, &key, |e| e.set_password(&secret)))
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        let key = key.to_string();
        self.guarded(move |slot| {
            Self::with_entry(slot, &key, |e| match e.get_password() {
                Ok(s) => Ok(Some(s)),
                Err(keyring_core::Error::NoEntry) => Ok(None),
                Err(e) => Err(e),
            })
        })
    }

    fn delete(&self, key: &str) -> Result<()> {
        let key = key.to_string();
        self.guarded(move |slot| {
            Self::with_entry(slot, &key, |e| match e.delete_credential() {
                Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
                Err(e) => Err(e),
            })
        })
    }
}

/// In-memory mock for tests. Can be made unavailable, or made to fail / corrupt
/// reads, to exercise the error paths. Its `Debug` never shows the secrets.
#[derive(Default)]
pub struct MemorySecretStore {
    available: AtomicBool,
    /// When true, `get` returns a changed value the first time it is called for
    /// a key (simulates a store that returns different data than it stored).
    corrupt_once: AtomicBool,
    /// When true, `put` fails (simulates a store that cannot write).
    fail_puts: AtomicBool,
    /// Every call sleeps this long first (simulates a hanging keyring).
    delay_ms: AtomicUsize,
    secrets: Mutex<HashMap<String, String>>,
}

impl std::fmt::Debug for MemorySecretStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let keys: Vec<String> = self
            .secrets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect();
        f.debug_struct("MemorySecretStore")
            .field("available", &self.available.load(Ordering::Relaxed))
            .field("keys", &keys)
            .finish_non_exhaustive()
    }
}

impl MemorySecretStore {
    pub fn new() -> Self {
        let s = Self::default();
        s.available.store(true, Ordering::Relaxed);
        s
    }

    /// A store that is present but refuses every operation (like a keyring
    /// that is unavailable).
    pub fn unavailable() -> Self {
        let s = Self::new();
        s.available.store(false, Ordering::Relaxed);
        s
    }

    /// Makes the store (un)available, like a keyring daemon stopping or starting.
    pub fn set_available(&self, on: bool) {
        self.available.store(on, Ordering::Relaxed);
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

    /// Makes every call sleep `d` first (a hanging keyring; 0 = off).
    pub fn delay(&self, d: Duration) {
        self.delay_ms
            .store(d.as_millis() as usize, Ordering::Relaxed);
    }

    /// Whether a secret is stored under `key` (bypasses availability, for assertions).
    pub fn contains(&self, key: &str) -> bool {
        self.secrets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(key)
    }

    fn fail_if_unavailable(&self) -> Result<()> {
        let ms = self.delay_ms.load(Ordering::Relaxed);
        if ms > 0 {
            std::thread::sleep(Duration::from_millis(ms as u64));
        }
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
        self.fail_if_unavailable().is_ok()
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
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    let shown = crate::redact::mask_url(url);
    let u =
        reqwest::Url::parse(url).map_err(|e| Error::invalid(format!("bad URL {shown}: {e}")))?;
    if u.username().is_empty() {
        return Ok(url.to_string());
    }
    let Some(scheme_end) = url.find("://") else {
        return Err(Error::invalid(format!("bad URL {shown}")));
    };
    let after_scheme = &url[scheme_end + 3..];
    let Some(at) = after_scheme.find('@') else {
        return Err(Error::invalid(format!("bad URL {shown}")));
    };
    Ok(format!(
        "{}{}",
        &url[..scheme_end + 3],
        &after_scheme[at + 1..]
    ))
}

/// Splits the userinfo off a URL: the URL without it, and an `Authorization: Basic`
/// header line built from the (percent-decoded) user name and password. URLs without
/// userinfo come back unchanged with `None`.
pub fn userinfo_to_basic_auth(url: &str) -> Result<(String, Option<String>)> {
    let shown = crate::redact::mask_url(url);
    let u =
        reqwest::Url::parse(url).map_err(|e| Error::invalid(format!("bad URL {shown}: {e}")))?;
    if u.username().is_empty() && u.password().is_none() {
        return Ok((url.to_string(), None));
    }
    let decode = |s: &str| {
        percent_encoding::percent_decode_str(s)
            .decode_utf8_lossy()
            .into_owned()
    };
    let header =
        crate::http::basic_auth_header(&decode(u.username()), &decode(u.password().unwrap_or("")));
    Ok((strip_url_userinfo(url)?, Some(header)))
}

/// Moves URL userinfo into an `Authorization` header (unless the job already has one),
/// so that it is sent with the probe *and* every segment request, and so that the URL
/// used for requests, redirects, logs and errors carries no password.
pub fn userinfo_into_headers(url: &mut String, headers: &mut Vec<String>) -> Result<()> {
    let (plain, auth) = userinfo_to_basic_auth(url)?;
    if let Some(auth) = auth {
        let has_auth = headers.iter().any(|h| {
            h.split_once(':')
                .is_some_and(|(n, _)| n.trim().eq_ignore_ascii_case("authorization"))
        });
        if !has_auth {
            headers.push(auth);
        }
        *url = plain;
    }
    Ok(())
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

    if migrated_jobs > 0 || migrated_proxy {
        // Best effort: scrub the cleared plaintext out of free pages and the WAL.
        let _ = db.compact();
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
    fn timeout_runner_returns_results_and_abandons_hung_calls() {
        let stuck = Arc::new(AtomicUsize::new(0));
        let t = Duration::from_millis(200);
        assert_eq!(run_with_timeout(t, &stuck, || Ok(7)).unwrap(), 7);

        // A call that hangs is abandoned with a clear "unavailable" error…
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let started = std::time::Instant::now();
        let err = run_with_timeout(t, &stuck, move || {
            let _ = release_rx.recv();
            Ok(())
        })
        .unwrap_err();
        assert!(matches!(err, Error::KeyringUnavailable(_)), "{err:?}");
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(stuck.load(Ordering::SeqCst), 1);

        // …and while it still hangs, further calls fail at once without a new thread.
        let started = std::time::Instant::now();
        let err = run_with_timeout(t, &stuck, || Ok(1)).unwrap_err();
        assert!(err.to_string().contains("not responding"), "{err}");
        assert!(started.elapsed() < Duration::from_millis(100));

        // Once the hung call returns, the store works again.
        release_tx.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while stuck.load(Ordering::SeqCst) > 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(run_with_timeout(t, &stuck, || Ok(2)).unwrap(), 2);
    }

    #[test]
    fn timeout_runner_survives_a_panicking_call() {
        let stuck = Arc::new(AtomicUsize::new(0));
        let r: Result<()> = run_with_timeout(Duration::from_secs(5), &stuck, || panic!("boom"));
        assert!(matches!(r, Err(Error::Keyring(_))));
        assert_eq!(stuck.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn userinfo_moves_into_an_authorization_header() {
        let mut url = "https://us%20er:pa%3Ass%40@[::1]:8443/p%20q?a=b@c".to_string();
        let mut headers = vec!["X-Other: 1".to_string()];
        userinfo_into_headers(&mut url, &mut headers).unwrap();
        assert_eq!(url, "https://[::1]:8443/p%20q?a=b@c");
        assert_eq!(
            headers[1],
            crate::http::basic_auth_header("us er", "pa:ss@")
        );
        // An explicit Authorization header wins; the userinfo is still removed.
        let mut url = "http://u:p@h/f".to_string();
        let mut headers = vec!["authorization: Bearer t".to_string()];
        userinfo_into_headers(&mut url, &mut headers).unwrap();
        assert_eq!((url.as_str(), headers.len()), ("http://h/f", 1));
        // No userinfo: unchanged.
        let mut url = "http://h/a@b".to_string();
        let mut headers = vec![];
        userinfo_into_headers(&mut url, &mut headers).unwrap();
        assert_eq!((url.as_str(), headers.len()), ("http://h/a@b", 0));
    }

    #[test]
    fn debug_output_never_shows_secrets() {
        let secret = JobSecret {
            url: "http://alice:DbgPw-DUMMY@h/f".into(),
            headers: vec!["Authorization: Basic DbgHdr-DUMMY".into()],
            cookie: Some("sid=DbgCookie-DUMMY".into()),
        };
        let store = MemorySecretStore::new();
        store.put("job-1", "DbgStored-DUMMY").unwrap();
        let job = Job {
            id: 1,
            url: secret.url.clone(),
            out_path: String::new(),
            state: crate::store::State::Queued,
            referer: Some("http://r:DbgRef-DUMMY@h/".into()),
            user_agent: None,
            headers: secret.headers.clone(),
            cookie: secret.cookie.clone(),
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
        let req = crate::AddRequest {
            url: secret.url.clone(),
            headers: secret.headers.clone(),
            cookie: secret.cookie.clone(),
            ..Default::default()
        };
        let new_job = crate::store::NewJob {
            url: secret.url.clone(),
            headers: secret.headers.clone(),
            cookie: secret.cookie.clone(),
            ..Default::default()
        };
        let settings = crate::Settings {
            proxy: "http://p:DbgProxy-DUMMY@px:3128".into(),
            ..Default::default()
        };
        let all = format!(
            "{secret:?} {job:?} {req:?} {new_job:?} {store:?} {settings:?} {:?}",
            KeyringSecretStore::new()
        );
        assert!(!all.contains("DUMMY"), "{all}");
        assert!(
            all.contains("alice:***@h") && all.contains("Authorization: ***"),
            "{all}"
        );
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
