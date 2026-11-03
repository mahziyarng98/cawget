//! Small helpers: file names, sizes, times, formatting.

use crate::error::{Error, Result};
use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike};
use percent_encoding::percent_decode_str;
use std::path::{Path, PathBuf};

/// Turns arbitrary text into a safe file name: no separators or control characters, no leading
/// dots, at most 200 bytes (never splitting a UTF-8 sequence), never empty.
pub fn sanitize_filename(input: &str) -> String {
    let mut s: String = input
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || c == '/' || c == '\\' {
                '_'
            } else {
                c
            }
        })
        .collect();
    if s.len() > 200 {
        let mut cut = 200;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
    }
    let s = s.trim_end().to_string();
    let lead = s.chars().take_while(|&c| c == '.').count();
    let s = format!("{}{}", "_".repeat(lead), &s[lead..]);
    if s.is_empty() { "download".into() } else { s }
}

/// Last path segment of a URL, percent-decoded and sanitised ("download" if there is none).
pub fn filename_from_url(url: &str) -> String {
    let after = url.split_once("://").map_or(url, |(_, r)| r);
    let path = after.find('/').map_or("", |i| &after[i..]);
    let path = path.split(['?', '#']).next().unwrap_or("");
    let last = path.rsplit('/').next().unwrap_or("");
    sanitize_filename(&percent_decode_str(last).decode_utf8_lossy())
}

/// Extracts the file name from a `Content-Disposition` header value (`filename*=` wins).
pub fn filename_from_disposition(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let mut raw: Option<String> = None;
    if let Some(i) = lower.find("filename*=") {
        let rest = &value[i + 10..];
        if let Some((_, enc)) = rest.split_once("''") {
            let enc = enc.split(';').next().unwrap_or("").trim();
            raw = Some(percent_decode_str(enc).decode_utf8_lossy().into_owned());
        }
    }
    if raw.is_none() {
        let mut from = 0;
        while let Some(i) = lower[from..].find("filename=") {
            let at = from + i;
            if at > 0 && lower.as_bytes()[at - 1] == b'*' {
                from = at + 9;
                continue;
            }
            let rest = value[at + 9..].trim_start();
            raw = Some(if let Some(q) = rest.strip_prefix('"') {
                q.split('"').next().unwrap_or("").to_string()
            } else {
                rest.split([';', ' ', '\t'])
                    .next()
                    .unwrap_or("")
                    .to_string()
            });
            break;
        }
    }
    let raw = raw.filter(|s| !s.is_empty())?;
    let base = raw.rsplit(['/', '\\']).next().unwrap_or(&raw);
    Some(sanitize_filename(base))
}

/// `name.ext` becomes `name (1).ext`, `name (2).ext`, ... until neither the file nor its `.part`
/// exists. A free name is returned unchanged.
pub fn unique_path(path: &Path) -> PathBuf {
    let taken = |p: &Path| {
        let mut part = p.as_os_str().to_owned();
        part.push(".part");
        p.symlink_metadata().is_ok() || Path::new(&part).symlink_metadata().is_ok()
    };
    if !taken(path) {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for k in 1..100_000 {
        let candidate = path.with_file_name(format!("{stem} ({k}){ext}"));
        if !taken(&candidate) {
            return candidate;
        }
    }
    path.to_path_buf()
}

/// "10", "4k", "1.5M", "2GiB" -> bytes (powers of 1024).
pub fn parse_size(s: &str) -> Result<u64> {
    let t = s.trim();
    let split = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(t.len());
    let (num, unit) = t.split_at(split);
    let v: f64 = num
        .parse()
        .map_err(|_| Error::invalid(format!("bad size '{s}'")))?;
    let unit = unit.trim().to_ascii_lowercase();
    let unit = unit.strip_suffix('b').unwrap_or(&unit);
    let unit = unit.strip_suffix('i').unwrap_or(unit);
    let mul: f64 = match unit {
        "" => 1.0,
        "k" => 1024.0,
        "m" => 1024.0 * 1024.0,
        "g" => 1024.0f64.powi(3),
        "t" => 1024.0f64.powi(4),
        _ => return Err(Error::invalid(format!("bad size unit in '{s}'"))),
    };
    Ok((v * mul) as u64)
}

/// Parses "HH:MM" into minutes since midnight.
pub fn parse_hhmm(s: &str) -> Option<u32> {
    let t = NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()?;
    Some(t.hour() * 60 + t.minute())
}

/// Is `minute` (of the day) inside `[start, end)`? A window with `start > end` wraps midnight;
/// `start == end` means "always".
pub fn in_window(minute: u32, start: u32, end: u32) -> bool {
    if start == end {
        true
    } else if start < end {
        (start..end).contains(&minute)
    } else {
        minute >= start || minute < end
    }
}

/// Parses a start time: `now`, `+90s`, `+10m`, `+2h`, `+1d`, `@epoch`, `HH:MM` (next occurrence,
/// local time) or `YYYY-MM-DD HH:MM[:SS]` (local time). Returns unix seconds.
pub fn parse_when(s: &str, now: i64) -> Result<i64> {
    let s = s.trim();
    let bad = || Error::invalid(format!("cannot parse time '{s}'"));
    if s == "now" {
        return Ok(now);
    }
    if let Some(e) = s.strip_prefix('@') {
        return e.parse().map_err(|_| bad());
    }
    if let Some(rel) = s.strip_prefix('+') {
        let split = rel.find(|c: char| !c.is_ascii_digit()).unwrap_or(rel.len());
        let (n, unit) = rel.split_at(split);
        let n: i64 = n.parse().map_err(|_| bad())?;
        let mul = match unit {
            "" | "s" => 1,
            "m" => 60,
            "h" => 3600,
            "d" => 86400,
            _ => return Err(bad()),
        };
        return Ok(now + n * mul);
    }
    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Local
                .from_local_datetime(&dt)
                .earliest()
                .map(|d| d.timestamp())
                .ok_or_else(bad);
        }
    }
    if let Some(minute) = parse_hhmm(s) {
        let nowdt = Local.timestamp_opt(now, 0).single().ok_or_else(bad)?;
        let today: NaiveDate = nowdt.date_naive();
        let at = |d: NaiveDate| {
            let dt = d.and_hms_opt(minute / 60, minute % 60, 0)?;
            Local
                .from_local_datetime(&dt)
                .earliest()
                .map(|x| x.timestamp())
        };
        let t = at(today).ok_or_else(bad)?;
        return if t > now {
            Ok(t)
        } else {
            at(today.succ_opt().ok_or_else(bad)?).ok_or_else(bad)
        };
    }
    Err(bad())
}

/// Minutes since local midnight for a unix timestamp.
pub fn local_minute_of_day(ts: i64) -> u32 {
    Local
        .timestamp_opt(ts, 0)
        .single()
        .map_or(0, |d| d.hour() * 60 + d.minute())
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// "1.5 MiB" style formatting.
pub fn fmt_bytes(v: f64) -> String {
    const U: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let (mut v, mut i) = (v, 0);
    while v >= 1024.0 && i < 4 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{v:.0} {}", U[i])
    } else {
        format!("{v:.1} {}", U[i])
    }
}

/// "01:15" / "1:02:05" / "--:--".
pub fn fmt_eta(secs: Option<f64>) -> String {
    match secs {
        Some(s) if (0.0..99.0 * 3600.0).contains(&s) => {
            let t = (s + 0.5) as u64;
            if t >= 3600 {
                format!("{}:{:02}:{:02}", t / 3600, (t / 60) % 60, t % 60)
            } else {
                format!("{:02}:{:02}", t / 60, t % 60)
            }
        }
        _ => "--:--".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_from_urls() {
        assert_eq!(
            filename_from_url("http://h/path/file.tar.gz?x=1#frag"),
            "file.tar.gz"
        );
        assert_eq!(
            filename_from_url("https://h/a/My%20File%2B1.zip"),
            "My File+1.zip"
        );
        assert_eq!(filename_from_url("http://h/"), "download");
        assert_eq!(filename_from_url("http://h"), "download");
        assert_eq!(filename_from_url("http://h/a/%2e%2e%2fpasswd"), "___passwd");
        assert!(!filename_from_url("http://h/%2e%2e").starts_with('.'));
    }

    #[test]
    fn names_from_content_disposition() {
        let f = filename_from_disposition;
        assert_eq!(
            f("attachment; filename=\"report final.bin\"").unwrap(),
            "report final.bin"
        );
        assert_eq!(
            f("attachment; filename=plain.txt; size=3").unwrap(),
            "plain.txt"
        );
        assert_eq!(
            f("attachment; filename*=UTF-8''na%C3%AFve%20file.txt").unwrap(),
            "naïve file.txt"
        );
        assert_eq!(
            f("attachment; filename=\"fallback.txt\"; filename*=UTF-8''better.txt").unwrap(),
            "better.txt"
        );
        assert_eq!(
            f("inline; filename=\"../../etc/passwd\"").unwrap(),
            "passwd"
        );
        assert_eq!(f("attachment; filename=\"a\\b.txt\"").unwrap(), "b.txt");
        assert!(f("attachment").is_none());
    }

    #[test]
    fn sanitising() {
        assert_eq!(sanitize_filename("  .hidden "), "_hidden");
        assert_eq!(sanitize_filename("a/b\\c\u{1}d"), "a_b_c_d");
        assert_eq!(sanitize_filename(""), "download");
        assert!(!sanitize_filename("...").starts_with('.'));
        assert!(sanitize_filename(&"é".repeat(500)).len() <= 200);
    }

    #[test]
    fn sizes() {
        assert_eq!(parse_size("0").unwrap(), 0);
        assert_eq!(parse_size("1024").unwrap(), 1024);
        assert_eq!(parse_size("4k").unwrap(), 4096);
        assert_eq!(parse_size("1.5M").unwrap(), 1_572_864);
        assert_eq!(parse_size("2GiB").unwrap(), 2_147_483_648);
        assert_eq!(parse_size("10KB").unwrap(), 10_240);
        for bad in ["", "abc", "5x", "-3", "5kbx"] {
            assert!(parse_size(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn times() {
        let now = 1_700_000_000;
        assert_eq!(parse_when("+90", now).unwrap(), now + 90);
        assert_eq!(parse_when("+10m", now).unwrap(), now + 600);
        assert_eq!(parse_when("+2h", now).unwrap(), now + 7200);
        assert_eq!(parse_when("+1d", now).unwrap(), now + 86400);
        assert_eq!(parse_when("@12345", now).unwrap(), 12345);
        assert_eq!(parse_when("now", now).unwrap(), now);
        let t = parse_when("2030-01-02 03:04", now).unwrap();
        let d = Local.timestamp_opt(t, 0).unwrap();
        assert_eq!((d.hour(), d.minute()), (3, 4));
        let hm = parse_when("23:59", now).unwrap();
        assert!(hm > now && hm <= now + 86400 + 3600);
        for bad in ["soon", "+5x", "25:00", ""] {
            assert!(parse_when(bad, now).is_err(), "{bad}");
        }
        assert_eq!(parse_hhmm("07:30"), Some(450));
        assert_eq!(parse_hhmm("24:00"), None);
    }

    #[test]
    fn daily_window() {
        assert!(in_window(100, 0, 0));
        assert!(in_window(60, 60, 120) && !in_window(120, 60, 120) && !in_window(59, 60, 120));
        assert!(in_window(23 * 60, 22 * 60, 6 * 60) && in_window(60, 22 * 60, 6 * 60));
        assert!(!in_window(12 * 60, 22 * 60, 6 * 60));
    }

    #[test]
    fn formatting() {
        assert_eq!(fmt_bytes(0.0), "0 B");
        assert_eq!(fmt_bytes(1536.0), "1.5 KiB");
        assert_eq!(fmt_bytes(5.0 * 1048576.0), "5.0 MiB");
        assert_eq!(fmt_eta(Some(75.0)), "01:15");
        assert_eq!(fmt_eta(Some(3725.0)), "1:02:05");
        assert_eq!(fmt_eta(None), "--:--");
    }

    #[test]
    fn unique_names_skip_files_and_parts() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.tar.gz");
        assert_eq!(unique_path(&p), p);
        std::fs::write(&p, b"x").unwrap();
        assert_eq!(unique_path(&p), dir.path().join("a.tar (1).gz"));
        std::fs::write(dir.path().join("a.tar (1).gz.part"), b"x").unwrap();
        assert_eq!(unique_path(&p), dir.path().join("a.tar (2).gz"));
    }
}
