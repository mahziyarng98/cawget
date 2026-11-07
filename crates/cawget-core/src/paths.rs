//! Where the queue database lives, the one-time copy from older locations, and private file permissions.
//!
//! The default database is `$XDG_DATA_HOME/cawget/cawget.db` (usually `~/.local/share/cawget/cawget.db`).
//! Earlier builds used `…/jackdaw/jackdaw.db` and, before that, `…/segget/segget.db`. On first start, if the new
//! file does not exist, the first of those legacy databases that exists (in that order) is **copied** (main file
//! plus write-ahead log, if any) to the new place. The old files are only read, never modified, moved or deleted.
//! If another process is still using that old database (its lock is held), nothing is copied and the old path is
//! used for this run; the copy happens on a later start.
//!
//! The database can hold credentials in plain text, so directories created here are private (0700) and database
//! files created here are 0600; SQLite gives its `-wal`/`-shm` files the mode of the database file. Files and
//! directories that already exist keep their mode.

use crate::error::{Error, Result};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Result of resolving the default database path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Migration {
    /// Nothing to do (no old database, or the new one already exists).
    NotNeeded,
    /// The old database was copied to the new location; the old files were left untouched.
    Copied { from: PathBuf },
    /// The old database is in use by another process; it is used in place for this run.
    Deferred { old: PathBuf },
}

/// Directory name and file name of the current default database.
pub const APP_DIR: &str = "cawget";
pub const DB_FILE: &str = "cawget.db";
/// Legacy default locations (directory, file name), newest first; the first one found is copied.
const LEGACY_DBS: [(&str, &str); 2] = [("jackdaw", "jackdaw.db"), ("segget", "segget.db")];

/// Environment variable naming the database.
pub const DB_ENV: &str = "CAWGET_DB";
/// Deprecated older names of [`DB_ENV`], still honoured, in order of precedence.
pub const LEGACY_DB_ENVS: [&str; 2] = ["JACKDAW_DB", "SEGGET_DB"];

/// Base data directory (`$XDG_DATA_HOME` or `~/.local/share`).
pub fn data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn default_db_in(base: &Path) -> PathBuf {
    base.join(APP_DIR).join(DB_FILE)
}

/// Legacy default databases under `base`, in the order they are looked for.
pub fn legacy_dbs_in(base: &Path) -> [PathBuf; 2] {
    LEGACY_DBS.map(|(dir, file)| base.join(dir).join(file))
}

/// The database named by the environment: `$CAWGET_DB`, else the first deprecated alias that is set.
/// Returns the path and the variable it came from (callers print a note for a deprecated one).
pub fn db_from_env() -> Option<(PathBuf, &'static str)> {
    db_from(|var| std::env::var_os(var))
}

fn db_from(get: impl Fn(&str) -> Option<OsString>) -> Option<(PathBuf, &'static str)> {
    std::iter::once(DB_ENV)
        .chain(LEGACY_DB_ENVS)
        .find_map(|var| {
            get(var)
                .filter(|p| !p.is_empty())
                .map(|p| (PathBuf::from(p), var))
        })
}

/// Creates `dir` and any missing parents; every directory created here is private (0700).
pub fn create_private_dir(dir: &Path) -> io::Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

/// Creates `path` as an empty private file (0600) unless it exists; an existing file is left as it is.
pub fn create_private_file(path: &Path) -> io::Result<()> {
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Err(e) if e.kind() != io::ErrorKind::AlreadyExists => Err(e),
        _ => Ok(()),
    }
}

/// Forces mode 0600 on an existing file (used for migration backups, which contain
/// the pre-migration plaintext).
pub fn set_private_file_mode(path: &Path) -> io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

/// The database to use when the user named none: the new default, migrating from a legacy location if needed.
/// Returns the path to open and what happened.
pub fn resolve_default_db() -> Result<(PathBuf, Migration)> {
    resolve_in(&data_dir())
}

/// Same as [`resolve_default_db`] for an explicit base directory (tests use a temporary one).
pub fn resolve_in(base: &Path) -> Result<(PathBuf, Migration)> {
    let new = default_db_in(base);
    if new.exists() {
        return Ok((new, Migration::NotNeeded));
    }
    let Some(old) = legacy_dbs_in(base).into_iter().find(|p| p.is_file()) else {
        return Ok((new, Migration::NotNeeded));
    };
    match migrate(&old, &new)? {
        Migration::Deferred { old } => Ok((old.clone(), Migration::Deferred { old })),
        m => Ok((new, m)),
    }
}

fn sidecar(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Copies `old` (and its `-wal`) to `new` if `old` exists and `new` does not. Never writes to `old`.
pub fn migrate(old: &Path, new: &Path) -> Result<Migration> {
    if new.exists() || !old.is_file() {
        return Ok(Migration::NotNeeded);
    }
    // If a scheduler holds the old lock, the database may be mid-write: do not copy now.
    // The lock file is only opened for reading; flock does not change its contents.
    let _guard = match File::open(sidecar(old, ".lock")) {
        Ok(f) => {
            match rustix::fs::flock(&f, rustix::fs::FlockOperation::NonBlockingLockExclusive) {
                Ok(()) => Some(f),
                Err(e) if e == rustix::io::Errno::WOULDBLOCK => {
                    return Ok(Migration::Deferred {
                        old: old.to_path_buf(),
                    });
                }
                Err(e) => return Err(Error::Io(e.into())),
            }
        }
        Err(_) => None, // no lock file: no scheduler ever ran on it
    };

    let dir = new
        .parent()
        .ok_or_else(|| Error::invalid("database path has no directory"))?;
    create_private_dir(dir)?;
    let staging = dir.join(".migrate-staging");
    if staging.exists() {
        fs::remove_dir_all(&staging)?; // leftovers of an interrupted copy (ours, never the old files)
    }
    fs::DirBuilder::new().mode(0o700).create(&staging)?;
    let name = new
        .file_name()
        .ok_or_else(|| Error::invalid("database path has no file name"))?;
    let staged = staging.join(name);
    // `fs::copy` carries the old file's mode over; the copies are made private (SQLite then gives the
    // `-shm` it creates the same mode).
    fs::copy(old, &staged)?;
    fs::set_permissions(&staged, fs::Permissions::from_mode(0o600))?;
    let old_wal = sidecar(old, "-wal");
    if old_wal.is_file() {
        let staged_wal = sidecar(&staged, "-wal");
        fs::copy(&old_wal, &staged_wal)?;
        fs::set_permissions(&staged_wal, fs::Permissions::from_mode(0o600))?;
    }
    // Check the copy (this also folds the copied WAL into it) before publishing it.
    {
        let conn = rusqlite::Connection::open(&staged)?;
        let ok: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if ok != "ok" {
            drop(conn);
            fs::remove_dir_all(&staging)?;
            return Err(Error::invalid(format!(
                "copy of {} failed the integrity check: {ok}",
                old.display()
            )));
        }
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    }
    for suffix in ["-wal", "-shm"] {
        let s = sidecar(&staged, suffix);
        if s.exists() {
            fs::remove_file(s)?;
        }
    }
    // Publishing is a rename inside one directory: the new database appears complete or not at all.
    fs::rename(&staged, new)?;
    fs::remove_dir(&staging)?;
    // Make sure the data reached the disk before we rely on it.
    OpenOptions::new().read(true).open(new)?.sync_all()?;
    Ok(Migration::Copied {
        from: old.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{NewJob, Store};
    use sha2::{Digest, Sha256};

    const JACKDAW: usize = 0; // index into `legacy_dbs_in`
    const SEGGET: usize = 1;

    fn digest(p: &Path) -> Option<String> {
        let data = fs::read(p).ok()?;
        Some(
            Sha256::digest(&data)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        )
    }

    fn snapshot(old: &Path) -> Vec<Option<String>> {
        ["", "-wal", "-shm", ".lock"]
            .iter()
            .map(|s| digest(&sidecar(old, s)))
            .collect()
    }

    fn mode(p: &Path) -> u32 {
        fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    fn make_old(base: &Path, which: usize, urls: &[&str]) -> PathBuf {
        let old = legacy_dbs_in(base)[which].clone();
        let st = Store::open(&old).unwrap();
        for u in urls {
            st.add_job(
                &NewJob {
                    url: (*u).into(),
                    ..Default::default()
                },
                1,
            )
            .unwrap();
        }
        st.set_setting("segments", "5").unwrap();
        old
    }

    fn urls(db: &Path) -> Vec<String> {
        let jobs = Store::open(db).unwrap().list_jobs(None).unwrap();
        jobs.into_iter().map(|j| j.url).collect()
    }

    #[test]
    fn copies_each_legacy_database_and_leaves_it_untouched() {
        for which in [JACKDAW, SEGGET] {
            let tmp = tempfile::tempdir().unwrap();
            let old = make_old(tmp.path(), which, &["http://a/1", "http://a/2"]);
            let before = snapshot(&old);
            let (path, m) = resolve_in(tmp.path()).unwrap();
            assert_eq!(path, default_db_in(tmp.path()));
            assert_eq!(m, Migration::Copied { from: old.clone() });
            assert_eq!(
                snapshot(&old),
                before,
                "old files must be byte-identical after the copy"
            );
            let st = Store::open(&path).unwrap();
            assert_eq!(st.list_jobs(None).unwrap().len(), 2);
            assert_eq!(st.get_setting("segments").unwrap().as_deref(), Some("5"));
            assert!(!path.parent().unwrap().join(".migrate-staging").exists());
            // second start: nothing to do, the copy is not repeated
            assert_eq!(resolve_in(tmp.path()).unwrap().1, Migration::NotNeeded);
        }
    }

    #[test]
    fn the_newer_legacy_database_wins_over_the_older_one() {
        let tmp = tempfile::tempdir().unwrap();
        let jackdaw = make_old(tmp.path(), JACKDAW, &["http://jackdaw"]);
        let segget = make_old(tmp.path(), SEGGET, &["http://segget"]);
        let (jb, sb) = (snapshot(&jackdaw), snapshot(&segget));
        let (path, m) = resolve_in(tmp.path()).unwrap();
        assert_eq!(
            m,
            Migration::Copied {
                from: jackdaw.clone()
            }
        );
        assert_eq!(urls(&path), vec!["http://jackdaw"]);
        assert_eq!((snapshot(&jackdaw), snapshot(&segget)), (jb, sb));
        assert_eq!(resolve_in(tmp.path()).unwrap().1, Migration::NotNeeded);
    }

    #[test]
    fn data_still_in_the_write_ahead_log_is_carried_over() {
        let tmp = tempfile::tempdir().unwrap();
        let old = legacy_dbs_in(tmp.path())[JACKDAW].clone();
        // Keep a connection open so the rows stay in the -wal file, as with a running old version.
        let st = Store::open(&old).unwrap();
        st.add_job(
            &NewJob {
                url: "http://wal/only".into(),
                ..Default::default()
            },
            1,
        )
        .unwrap();
        assert!(
            sidecar(&old, "-wal")
                .metadata()
                .map(|m| m.len() > 0)
                .unwrap_or(false),
            "test needs a non-empty WAL"
        );
        let before = snapshot(&old);
        let (path, m) = resolve_in(tmp.path()).unwrap();
        assert!(matches!(m, Migration::Copied { .. }));
        assert_eq!(snapshot(&old), before);
        assert_eq!(urls(&path), vec!["http://wal/only"]);
        drop(st);
    }

    #[test]
    fn existing_new_database_wins_and_nothing_is_copied() {
        let tmp = tempfile::tempdir().unwrap();
        make_old(tmp.path(), JACKDAW, &["http://jackdaw"]);
        make_old(tmp.path(), SEGGET, &["http://segget"]);
        let new = default_db_in(tmp.path());
        Store::open(&new)
            .unwrap()
            .add_job(
                &NewJob {
                    url: "http://new".into(),
                    ..Default::default()
                },
                1,
            )
            .unwrap();
        let (path, m) = resolve_in(tmp.path()).unwrap();
        assert_eq!((path.clone(), m), (new, Migration::NotNeeded));
        assert_eq!(urls(&path), vec!["http://new"]);
    }

    #[test]
    fn no_legacy_database_means_a_fresh_new_one() {
        let tmp = tempfile::tempdir().unwrap();
        let (path, m) = resolve_in(tmp.path()).unwrap();
        assert_eq!((path, m), (default_db_in(tmp.path()), Migration::NotNeeded));
        for old in legacy_dbs_in(tmp.path()) {
            assert!(
                !old.parent().unwrap().exists(),
                "a legacy location is never created"
            );
        }
    }

    #[test]
    fn legacy_database_in_use_is_left_alone_and_used_in_place() {
        let tmp = tempfile::tempdir().unwrap();
        let old = make_old(tmp.path(), JACKDAW, &["http://busy"]);
        let older = make_old(tmp.path(), SEGGET, &["http://segget"]);
        let lock = File::create(sidecar(&old, ".lock")).unwrap();
        rustix::fs::flock(&lock, rustix::fs::FlockOperation::LockExclusive).unwrap(); // "old version running"
        let (before, older_before) = (snapshot(&old), snapshot(&older));
        let (path, m) = resolve_in(tmp.path()).unwrap();
        assert_eq!(
            path, old,
            "the busy newer database is used, not the older one"
        );
        assert_eq!(m, Migration::Deferred { old: old.clone() });
        assert!(!default_db_in(tmp.path()).exists());
        assert_eq!((snapshot(&old), snapshot(&older)), (before, older_before));
        drop(lock);
        assert_eq!(
            resolve_in(tmp.path()).unwrap().1,
            Migration::Copied { from: old },
            "copied once the lock is free"
        );
    }

    #[test]
    fn the_copy_is_private_even_when_the_legacy_files_are_not() {
        let tmp = tempfile::tempdir().unwrap();
        let old = make_old(tmp.path(), SEGGET, &["http://a/1"]);
        fs::set_permissions(old.parent().unwrap(), fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&old, fs::Permissions::from_mode(0o644)).unwrap();
        let (path, _) = resolve_in(tmp.path()).unwrap();
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&old), 0o644, "the legacy file keeps its mode");
    }

    #[test]
    fn new_database_files_and_directories_are_private() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("a/b/q.db");
        let st = Store::open(&db).unwrap();
        st.set_setting("segments", "4").unwrap(); // writes, so the -wal and -shm files exist
        assert_eq!(mode(&tmp.path().join("a")), 0o700);
        assert_eq!(mode(db.parent().unwrap()), 0o700);
        for suffix in ["", "-wal", "-shm"] {
            assert_eq!(mode(&sidecar(&db, suffix)), 0o600, "{suffix}");
        }
    }

    #[test]
    fn existing_files_keep_their_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("shared.db");
        fs::write(&db, b"").unwrap();
        fs::set_permissions(&db, fs::Permissions::from_mode(0o640)).unwrap();
        Store::open(&db).unwrap();
        assert_eq!(mode(&db), 0o640);
    }

    #[test]
    fn database_variable_and_its_deprecated_aliases_in_order() {
        let env = |set: &[(&str, &str)]| {
            db_from(|var| {
                set.iter()
                    .find(|(k, _)| *k == var)
                    .map(|(_, v)| OsString::from(v))
            })
        };
        let all = [
            ("SEGGET_DB", "/s"),
            ("JACKDAW_DB", "/j"),
            ("CAWGET_DB", "/c"),
        ];
        assert_eq!(env(&all), Some((PathBuf::from("/c"), "CAWGET_DB")));
        assert_eq!(env(&all[..2]), Some((PathBuf::from("/j"), "JACKDAW_DB")));
        assert_eq!(env(&all[..1]), Some((PathBuf::from("/s"), "SEGGET_DB")));
        assert_eq!(
            env(&[("CAWGET_DB", ""), ("SEGGET_DB", "/s")]),
            Some((PathBuf::from("/s"), "SEGGET_DB")),
            "an empty value counts as unset"
        );
        assert_eq!(env(&[]), None);
    }
}
