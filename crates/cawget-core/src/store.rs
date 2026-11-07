//! SQLite persistence: settings, jobs and per-segment progress.
//!
//! One connection behind a mutex, WAL journal, so a CLI process and a GUI/daemon process can use
//! the same database file at the same time.

use crate::error::{Error, Result};
use crate::segment::Segment;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

/// Lifecycle of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Queued,
    Active,
    Paused,
    Done,
    Failed,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Queued => "queued",
            State::Active => "active",
            State::Paused => "paused",
            State::Done => "done",
            State::Failed => "failed",
        }
    }
    fn parse(s: &str) -> State {
        match s {
            "active" => State::Active,
            "paused" => State::Paused,
            "done" => State::Done,
            "failed" => State::Failed,
            _ => State::Queued,
        }
    }
}

/// A download job as stored in the database. `None` means "not set / not known yet".
/// `Debug` hides credentials (URL userinfo, header values, cookie).
#[derive(Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: i64,
    pub url: String,
    /// Final path. A path ending in `/` is a directory: the file name is chosen on first contact.
    pub out_path: String,
    pub state: State,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    /// Extra request headers, `Name: value`.
    pub headers: Vec<String>,
    pub cookie: Option<String>,
    /// Keyring reference of this job's credential secret (empty = none). Credentials
    /// themselves never live in the database.
    #[serde(skip)]
    pub secret_ref: String,
    /// Expected SHA-256 (hex).
    pub sha256: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub error: Option<String>,
    pub size: Option<u64>,
    pub accept_ranges: bool,
    /// Requested connections for this job (0 = use the setting).
    pub segments: u32,
    pub created: i64,
    /// Unix time before which the job must not start (0 = immediately).
    pub not_before: i64,
}

/// A job row that still carries plaintext credentials in the database.
#[derive(Clone)]
pub struct PlaintextJob {
    pub id: i64,
    pub url: String,
    pub headers: Vec<String>,
    pub cookie: Option<String>,
}

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use crate::redact::{RedactedHeaders, mask_url, opt_secret};
        f.debug_struct("Job")
            .field("id", &self.id)
            .field("url", &mask_url(&self.url))
            .field("out_path", &self.out_path)
            .field("state", &self.state)
            .field("referer", &self.referer.as_deref().map(mask_url))
            .field("user_agent", &self.user_agent)
            .field("headers", &RedactedHeaders(&self.headers))
            .field("cookie", &opt_secret(&self.cookie))
            .field("secret_ref", &self.secret_ref)
            .field("sha256", &self.sha256)
            .field("etag", &self.etag)
            .field("last_modified", &self.last_modified)
            .field("error", &self.error)
            .field("size", &self.size)
            .field("accept_ranges", &self.accept_ranges)
            .field("segments", &self.segments)
            .field("created", &self.created)
            .field("not_before", &self.not_before)
            .finish()
    }
}

impl std::fmt::Debug for PlaintextJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use crate::redact::{RedactedHeaders, mask_url, opt_secret};
        f.debug_struct("PlaintextJob")
            .field("id", &self.id)
            .field("url", &mask_url(&self.url))
            .field("headers", &RedactedHeaders(&self.headers))
            .field("cookie", &opt_secret(&self.cookie))
            .finish()
    }
}

impl std::fmt::Debug for NewJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use crate::redact::{RedactedHeaders, mask_url, opt_secret};
        f.debug_struct("NewJob")
            .field("url", &mask_url(&self.url))
            .field("out_path", &self.out_path)
            .field("paused", &self.paused)
            .field("referer", &self.referer.as_deref().map(mask_url))
            .field("user_agent", &self.user_agent)
            .field("headers", &RedactedHeaders(&self.headers))
            .field("cookie", &opt_secret(&self.cookie))
            .field("sha256", &self.sha256)
            .field("segments", &self.segments)
            .field("not_before", &self.not_before)
            .finish()
    }
}

/// What the caller supplies when creating a job.
#[derive(Clone, Default)]
pub struct NewJob {
    pub url: String,
    pub out_path: String,
    pub paused: bool,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    pub headers: Vec<String>,
    pub cookie: Option<String>,
    pub sha256: Option<String>,
    pub segments: u32,
    pub not_before: i64,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS jobs(
  id INTEGER PRIMARY KEY AUTOINCREMENT, url TEXT NOT NULL, out_path TEXT NOT NULL DEFAULT '',
  state TEXT NOT NULL DEFAULT 'queued', referer TEXT NOT NULL DEFAULT '',
  user_agent TEXT NOT NULL DEFAULT '', headers TEXT NOT NULL DEFAULT '',
  cookie TEXT NOT NULL DEFAULT '', sha256 TEXT NOT NULL DEFAULT '',
  etag TEXT NOT NULL DEFAULT '', last_modified TEXT NOT NULL DEFAULT '',
  error TEXT NOT NULL DEFAULT '', size INTEGER NOT NULL DEFAULT -1,
  accept_ranges INTEGER NOT NULL DEFAULT 0, nseg INTEGER NOT NULL DEFAULT 0,
  created INTEGER NOT NULL DEFAULT 0, not_before INTEGER NOT NULL DEFAULT 0,
  secret_ref TEXT NOT NULL DEFAULT '');
CREATE TABLE IF NOT EXISTS segments(
  job_id INTEGER NOT NULL REFERENCES jobs(id) ON DELETE CASCADE, idx INTEGER NOT NULL,
  start INTEGER NOT NULL, end INTEGER NOT NULL, done INTEGER NOT NULL,
  PRIMARY KEY(job_id, idx));
";

// Column order shared by every query that returns a job.
const COLS: &str = "id,url,out_path,state,referer,user_agent,headers,cookie,sha256,etag,\
                    last_modified,error,size,accept_ranges,nseg,created,not_before,secret_ref";

fn opt(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

fn row_to_job(r: &Row) -> rusqlite::Result<Job> {
    let size: i64 = r.get(12)?;
    let headers: String = r.get(6)?;
    Ok(Job {
        id: r.get(0)?,
        url: r.get(1)?,
        out_path: r.get(2)?,
        state: State::parse(&r.get::<_, String>(3)?),
        referer: opt(r.get(4)?),
        user_agent: opt(r.get(5)?),
        headers: headers
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect(),
        cookie: opt(r.get(7)?),
        sha256: opt(r.get(8)?),
        etag: opt(r.get(9)?),
        last_modified: opt(r.get(10)?),
        error: opt(r.get(11)?),
        size: u64::try_from(size).ok(),
        accept_ranges: r.get::<_, i64>(13)? != 0,
        segments: r.get::<_, i64>(14)? as u32,
        created: r.get(15)?,
        not_before: r.get(16)?,
        secret_ref: r.get(17)?,
    })
}

/// Database handle. Cheap to share behind an `Arc`.
pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    /// Opens (creating if needed) the database file, creating parent directories. Whatever is created here is
    /// private (directories 0700, the file 0600), since the queue can hold credentials.
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            crate::paths::create_private_dir(dir)?;
        }
        crate::paths::create_private_file(path)?;
        Self::init(Connection::open(path)?)
    }

    /// In-memory database (tests).
    pub fn open_in_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        // `journal_mode` returns a row, so it must be read, not executed.
        let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL;")?;
        conn.execute_batch(SCHEMA)?;
        // Databases created by older versions lack the `secret_ref` column.
        let has_ref = {
            let mut st = conn.prepare("PRAGMA table_info(jobs)")?;
            st.query_map([], |r| r.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .iter()
                .any(|c| c == "secret_ref")
        };
        if !has_ref {
            conn.execute(
                "ALTER TABLE jobs ADD COLUMN secret_ref TEXT NOT NULL DEFAULT ''",
                [],
            )?;
        }
        Ok(Store {
            conn: Mutex::new(conn),
        })
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---- settings ----

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.db().execute(
            "INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .db()
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn unset_setting(&self, key: &str) -> Result<()> {
        self.db()
            .execute("DELETE FROM settings WHERE key=?1", [key])?;
        Ok(())
    }

    pub fn all_settings(&self) -> Result<HashMap<String, String>> {
        let db = self.db();
        let mut st = db.prepare("SELECT key,value FROM settings")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ---- jobs ----

    pub fn add_job(&self, j: &NewJob, created: i64) -> Result<i64> {
        let db = self.db();
        db.execute(
            "INSERT INTO jobs(url,out_path,state,referer,user_agent,headers,cookie,sha256,nseg,created,not_before)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                j.url,
                j.out_path,
                if j.paused { "paused" } else { "queued" },
                j.referer.as_deref().unwrap_or(""),
                j.user_agent.as_deref().unwrap_or(""),
                j.headers.join("\n"),
                j.cookie.as_deref().unwrap_or(""),
                j.sha256.as_deref().unwrap_or(""),
                j.segments,
                created,
                j.not_before,
            ],
        )?;
        Ok(db.last_insert_rowid())
    }

    pub fn get_job(&self, id: i64) -> Result<Option<Job>> {
        let sql = format!("SELECT {COLS} FROM jobs WHERE id=?1");
        Ok(self.db().query_row(&sql, [id], row_to_job).optional()?)
    }

    pub fn list_jobs(&self, state: Option<State>) -> Result<Vec<Job>> {
        let db = self.db();
        let (sql, arg) = match state {
            Some(s) => (
                format!("SELECT {COLS} FROM jobs WHERE state=?1 ORDER BY id"),
                s.as_str(),
            ),
            None => (
                format!("SELECT {COLS} FROM jobs WHERE ?1='' ORDER BY id"),
                "",
            ),
        };
        let mut st = db.prepare(&sql)?;
        let rows = st.query_map([arg], row_to_job)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn job_state(&self, id: i64) -> Result<Option<State>> {
        let s: Option<String> = self
            .db()
            .query_row("SELECT state FROM jobs WHERE id=?1", [id], |r| r.get(0))
            .optional()?;
        Ok(s.map(|s| State::parse(&s)))
    }

    pub fn set_state(&self, id: i64, state: State, error: Option<&str>) -> Result<()> {
        self.db().execute(
            "UPDATE jobs SET state=?2, error=?3 WHERE id=?1",
            params![id, state.as_str(), error.unwrap_or("")],
        )?;
        Ok(())
    }

    /// Saves what was learned from the server (path, size, validators, range support).
    pub fn set_meta(&self, j: &Job) -> Result<()> {
        self.db().execute(
            "UPDATE jobs SET out_path=?2,size=?3,etag=?4,last_modified=?5,accept_ranges=?6 WHERE id=?1",
            params![
                j.id,
                j.out_path,
                j.size.map_or(-1, |s| s as i64),
                j.etag.as_deref().unwrap_or(""),
                j.last_modified.as_deref().unwrap_or(""),
                j.accept_ranges,
            ],
        )?;
        Ok(())
    }

    pub fn set_schedule(&self, id: i64, not_before: i64) -> Result<()> {
        self.db().execute(
            "UPDATE jobs SET not_before=?2 WHERE id=?1",
            params![id, not_before],
        )?;
        Ok(())
    }

    pub fn delete_job(&self, id: i64) -> Result<()> {
        self.db().execute("DELETE FROM jobs WHERE id=?1", [id])?;
        Ok(())
    }

    /// Replaces the URL and clears the stored headers/cookie of a job, recording its
    /// keyring reference. Used when credentials are moved to the keyring (the URL is
    /// stored without userinfo).
    pub fn set_credentials(
        &self,
        id: i64,
        url: &str,
        headers: &[String],
        cookie: Option<&str>,
        secret_ref: &str,
    ) -> Result<()> {
        self.db().execute(
            "UPDATE jobs SET url=?2, headers=?3, cookie=?4, secret_ref=?5 WHERE id=?1",
            params![
                id,
                url,
                headers.join("\n"),
                cookie.unwrap_or(""),
                secret_ref
            ],
        )?;
        Ok(())
    }

    /// All queued jobs whose stored row still carries credentials (headers, cookie or
    /// URL userinfo). These are the rows the keyring migration must convert.
    pub fn plaintext_jobs(&self) -> Result<Vec<PlaintextJob>> {
        let db = self.db();
        let mut st = db.prepare(
            "SELECT id,url,headers,cookie FROM jobs WHERE headers <> '' OR cookie <> '' OR instr(url,'@') > 0",
        )?;
        let rows = st.query_map([], |r| {
            let headers: String = r.get(2)?;
            Ok(PlaintextJob {
                id: r.get(0)?,
                url: r.get(1)?,
                headers: headers
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(str::to_string)
                    .collect(),
                cookie: opt(r.get(3)?),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Rewrites the database file and truncates the WAL, so that values cleared by an
    /// `UPDATE` (the plaintext credentials of the keyring migration) do not linger in
    /// free space or in old WAL frames.
    pub fn compact(&self) -> Result<()> {
        let db = self.db();
        db.execute_batch("VACUUM")?;
        db.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        Ok(())
    }

    /// The stored proxy setting, if any (the value may carry userinfo in old,
    /// un-migrated databases).
    pub fn proxy_setting(&self) -> Result<Option<String>> {
        Ok(self
            .db()
            .query_row("SELECT value FROM settings WHERE key='proxy'", [], |r| {
                r.get(0)
            })
            .optional()?)
    }

    /// Jobs left `active` by a crashed process become `queued` again.
    pub fn reset_active(&self) -> Result<usize> {
        Ok(self
            .db()
            .execute("UPDATE jobs SET state='queued' WHERE state='active'", [])?)
    }

    // ---- segments ----

    /// Replaces the saved segments of a job atomically.
    pub fn save_segments(&self, id: i64, segs: &[Segment]) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction()?;
        tx.execute("DELETE FROM segments WHERE job_id=?1", [id])?;
        {
            let mut ins = tx.prepare(
                "INSERT INTO segments(job_id,idx,start,end,done) VALUES(?1,?2,?3,?4,?5)",
            )?;
            for (i, s) in segs.iter().enumerate() {
                ins.execute(params![
                    id,
                    i as i64,
                    s.start as i64,
                    s.end as i64,
                    s.done as i64
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn load_segments(&self, id: i64) -> Result<Vec<Segment>> {
        let db = self.db();
        let mut st =
            db.prepare("SELECT start,end,done FROM segments WHERE job_id=?1 ORDER BY idx")?;
        let rows = st.query_map([id], |r| {
            Ok(Segment {
                start: r.get::<_, i64>(0)? as u64,
                end: r.get::<_, i64>(1)? as u64,
                done: r.get::<_, i64>(2)? as u64,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Bytes saved for every job that has segments.
    pub fn done_bytes_all(&self) -> Result<HashMap<i64, u64>> {
        let db = self.db();
        let mut st = db.prepare("SELECT job_id, SUM(done) FROM segments GROUP BY job_id")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn done_bytes(&self, id: i64) -> Result<u64> {
        let v: i64 = self.db().query_row(
            "SELECT COALESCE(SUM(done),0) FROM segments WHERE job_id=?1",
            [id],
            |r| r.get(0),
        )?;
        Ok(v as u64)
    }

    /// Fails with `NoSuchJob` unless the job exists.
    pub fn require_job(&self, id: i64) -> Result<Job> {
        self.get_job(id)?.ok_or(Error::NoSuchJob(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_job(url: &str) -> NewJob {
        NewJob {
            url: url.into(),
            out_path: "/tmp/dl/".into(),
            segments: 6,
            ..Default::default()
        }
    }

    #[test]
    fn settings_roundtrip_and_overwrite() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.get_setting("segments").unwrap(), None);
        s.set_setting("segments", "12").unwrap();
        s.set_setting("segments", "16").unwrap();
        assert_eq!(s.get_setting("segments").unwrap().as_deref(), Some("16"));
        assert_eq!(s.all_settings().unwrap().len(), 1);
        s.unset_setting("segments").unwrap();
        assert_eq!(s.get_setting("segments").unwrap(), None);
    }

    #[test]
    fn job_roundtrip_and_meta_update() {
        let s = Store::open_in_memory().unwrap();
        let mut nj = new_job("https://example.org/a b.bin?x=1");
        nj.headers = vec!["X-A: 1".into(), "X-B: 2".into()];
        nj.sha256 = Some("ab".repeat(32));
        nj.not_before = 2000;
        let id = s.add_job(&nj, 1000).unwrap();
        let mut j = s.require_job(id).unwrap();
        assert_eq!(
            (j.state, j.size, j.segments, j.not_before, j.created),
            (State::Queued, None, 6, 2000, 1000)
        );
        assert_eq!(j.headers, vec!["X-A: 1", "X-B: 2"]);
        assert!(j.etag.is_none() && j.error.is_none());

        j.out_path = "/tmp/dl/a b.bin".into();
        j.etag = Some("\"abc\"".into());
        j.size = Some(12345);
        j.accept_ranges = true;
        s.set_meta(&j).unwrap();
        s.set_state(id, State::Failed, Some("HTTP 403")).unwrap();
        let h = s.require_job(id).unwrap();
        assert_eq!(
            (h.out_path.as_str(), h.size, h.accept_ranges),
            ("/tmp/dl/a b.bin", Some(12345), true)
        );
        assert_eq!(
            (h.etag.as_deref(), h.error.as_deref(), h.state),
            (Some("\"abc\""), Some("HTTP 403"), State::Failed)
        );
        assert!(matches!(s.require_job(id + 9), Err(Error::NoSuchJob(_))));
    }

    #[test]
    fn segments_persist_replace_and_cascade() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.db");
        let id;
        let segs = vec![
            Segment {
                start: 0,
                end: 99,
                done: 100,
            },
            Segment {
                start: 100,
                end: 199,
                done: 37,
            },
            Segment {
                start: 200,
                end: 299,
                done: 0,
            },
            Segment {
                start: 300,
                end: 399,
                done: 99,
            },
        ];
        {
            let s = Store::open(&path).unwrap();
            id = s.add_job(&new_job("http://h/f"), 1).unwrap();
            s.save_segments(id, &segs).unwrap();
        } // "process dies": handle dropped, state stays on disk
        let s = Store::open(&path).unwrap();
        assert_eq!(s.load_segments(id).unwrap(), segs);
        assert_eq!(s.done_bytes(id).unwrap(), 236);
        assert_eq!(s.done_bytes_all().unwrap()[&id], 236);
        let mut fewer = segs[..3].to_vec();
        fewer[1].done = 80;
        s.save_segments(id, &fewer).unwrap();
        assert_eq!(s.load_segments(id).unwrap(), fewer);
        s.delete_job(id).unwrap();
        assert!(
            s.load_segments(id).unwrap().is_empty(),
            "segments cascade with the job"
        );
    }

    #[test]
    fn crashed_active_jobs_are_requeued_and_listing_filters() {
        let s = Store::open_in_memory().unwrap();
        let a = s.add_job(&new_job("http://h/a"), 1).unwrap();
        let b = s
            .add_job(
                &NewJob {
                    paused: true,
                    ..new_job("http://h/b")
                },
                1,
            )
            .unwrap();
        s.set_state(a, State::Active, None).unwrap();
        assert_eq!(s.reset_active().unwrap(), 1);
        assert_eq!(s.job_state(a).unwrap(), Some(State::Queued));
        assert_eq!(s.job_state(b).unwrap(), Some(State::Paused));
        assert_eq!(s.list_jobs(None).unwrap().len(), 2);
        let q = s.list_jobs(Some(State::Queued)).unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].id, a);
    }
}
