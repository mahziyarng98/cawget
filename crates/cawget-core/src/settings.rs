//! User-tunable settings, stored as key/value text in the database.

use crate::error::{Error, Result};
use crate::store::Store;
use crate::util::{parse_hhmm, parse_size};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Documentation of one setting (used by `cawget-cli config` and the app).
pub struct SettingInfo {
    pub key: &'static str,
    pub default: &'static str,
    pub help: &'static str,
}

pub const SETTINGS: &[SettingInfo] = &[
    SettingInfo {
        key: "proxy",
        default: "",
        help: "proxy: empty = system (environment variables), none = direct, or http://, https://, socks5:// URL",
    },
    SettingInfo {
        key: "dir_compressed",
        default: "",
        help: "folder for compressed files (empty = default folder)",
    },
    SettingInfo {
        key: "dir_documents",
        default: "",
        help: "folder for documents (empty = default folder)",
    },
    SettingInfo {
        key: "dir_music",
        default: "",
        help: "folder for music (empty = default folder)",
    },
    SettingInfo {
        key: "dir_programs",
        default: "",
        help: "folder for programs (empty = default folder)",
    },
    SettingInfo {
        key: "dir_video",
        default: "",
        help: "folder for video (empty = default folder)",
    },
    SettingInfo {
        key: "segments",
        default: "8",
        help: "connections per file (1-64)",
    },
    SettingInfo {
        key: "max_jobs",
        default: "3",
        help: "files downloaded at the same time",
    },
    SettingInfo {
        key: "min_segment_size",
        default: "1M",
        help: "smallest piece a file is split into",
    },
    SettingInfo {
        key: "max_speed",
        default: "0",
        help: "total speed limit in bytes/s (0 = unlimited)",
    },
    SettingInfo {
        key: "retries",
        default: "5",
        help: "retries per segment before a job fails",
    },
    SettingInfo {
        key: "connect_timeout",
        default: "15",
        help: "seconds",
    },
    SettingInfo {
        key: "cacert",
        default: "",
        help: "extra CA bundle (PEM); empty = system trust store only",
    },
    SettingInfo {
        key: "user_agent",
        default: crate::DEFAULT_USER_AGENT,
        help: "User-Agent header",
    },
    SettingInfo {
        key: "download_dir",
        default: "",
        help: "default folder (empty = ~/Downloads or current directory)",
    },
    SettingInfo {
        key: "window",
        default: "",
        help: "daily start window HH:MM-HH:MM (empty = always)",
    },
];

/// Parsed settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub segments: u32,
    pub max_jobs: u32,
    pub min_segment_size: u64,
    pub max_speed: u64,
    pub retries: u32,
    pub connect_timeout: u64,
    pub cacert: Option<PathBuf>,
    pub user_agent: String,
    pub download_dir: Option<PathBuf>,
    /// Daily window in minutes since midnight, `(start, end)`.
    pub window: Option<(u32, u32)>,
    /// Folder per download category id (see `categories`); a missing entry means the default folder.
    pub category_dirs: BTreeMap<String, PathBuf>,
    /// "" = system proxy (environment variables), "none" = direct connections, otherwise a proxy URL.
    pub proxy: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            segments: 8,
            max_jobs: 3,
            min_segment_size: 1 << 20,
            max_speed: 0,
            retries: 5,
            connect_timeout: 15,
            cacert: None,
            user_agent: crate::DEFAULT_USER_AGENT.into(),
            download_dir: None,
            window: None,
            category_dirs: BTreeMap::new(),
            proxy: String::new(),
        }
    }
}

fn parse_window(v: &str) -> Result<Option<(u32, u32)>> {
    if v.trim().is_empty() {
        return Ok(None);
    }
    let bad = || Error::invalid("window: expected HH:MM-HH:MM");
    let (a, b) = v.split_once('-').ok_or_else(bad)?;
    Ok(Some((
        parse_hhmm(a).ok_or_else(bad)?,
        parse_hhmm(b).ok_or_else(bad)?,
    )))
}

/// "", "none", or an absolute proxy URL with a supported scheme.
fn parse_proxy(v: &str) -> Result<String> {
    let v = v.trim();
    if v.is_empty() || v == "none" {
        return Ok(v.to_string());
    }
    let ok = ["http://", "https://", "socks5://", "socks5h://"]
        .iter()
        .any(|p| v.to_ascii_lowercase().starts_with(p))
        && reqwest::Url::parse(v)
            .map(|u| u.host_str().is_some())
            .unwrap_or(false);
    if ok {
        Ok(v.to_string())
    } else {
        Err(Error::invalid(
            "proxy: expected empty (system), none, or a URL like http://host:3128 or socks5://host:1080",
        ))
    }
}

fn int_in(key: &str, v: &str, lo: u64, hi: u64) -> Result<u64> {
    v.trim()
        .parse::<u64>()
        .ok()
        .filter(|n| (lo..=hi).contains(n))
        .ok_or_else(|| Error::invalid(format!("{key}: expected an integer {lo}..{hi}")))
}

impl Settings {
    /// Applies one `key = value` pair; unknown keys and bad values are errors.
    pub fn apply(&mut self, key: &str, v: &str) -> Result<()> {
        match key {
            "segments" => self.segments = int_in(key, v, 1, 64)? as u32,
            "max_jobs" => self.max_jobs = int_in(key, v, 1, 100)? as u32,
            "retries" => self.retries = int_in(key, v, 0, 100)? as u32,
            "connect_timeout" => self.connect_timeout = int_in(key, v, 1, 600)?,
            "min_segment_size" => {
                self.min_segment_size = parse_size(v).ok().filter(|&n| n >= 1).ok_or_else(|| {
                    Error::invalid("min_segment_size: expected a size like 512k or 4M")
                })?
            }
            "max_speed" => {
                self.max_speed = parse_size(v)
                    .map_err(|_| Error::invalid("max_speed: expected a size like 500k or 2M"))?
            }
            "cacert" => self.cacert = (!v.is_empty()).then(|| PathBuf::from(v)),
            "user_agent" => {
                self.user_agent = if v.is_empty() {
                    Settings::default().user_agent
                } else {
                    v.into()
                }
            }
            "download_dir" => self.download_dir = (!v.is_empty()).then(|| PathBuf::from(v)),
            "window" => self.window = parse_window(v)?,
            "proxy" => self.proxy = parse_proxy(v)?,
            k if k.starts_with("dir_")
                && crate::categories::all().iter().any(|c| c.id == k[4..]) =>
            {
                if v.is_empty() {
                    self.category_dirs.remove(&k[4..]);
                } else {
                    self.category_dirs
                        .insert(k[4..].to_string(), PathBuf::from(v));
                }
            }
            _ => return Err(Error::invalid(format!("unknown setting '{key}'"))),
        }
        Ok(())
    }

    /// Defaults overridden by whatever is stored. Invalid stored values are ignored.
    pub fn load(store: &Store) -> Settings {
        let mut s = Settings::default();
        if let Ok(all) = store.all_settings() {
            for (k, v) in all {
                let _ = s.apply(&k, &v);
            }
        }
        s
    }

    /// Validates and stores a value (an empty value resets to the default).
    pub fn set(store: &Store, key: &str, value: &str) -> Result<()> {
        if value.is_empty() {
            if !SETTINGS.iter().any(|i| i.key == key) {
                return Err(Error::invalid(format!("unknown setting '{key}'")));
            }
            return store.unset_setting(key);
        }
        Settings::default().apply(key, value)?;
        store.set_setting(key, value)
    }

    /// The folder a file of this name goes to when the user picked none: its category folder if one is set,
    /// otherwise the default folder.
    pub fn folder_for(&self, file_name: &str) -> PathBuf {
        crate::categories::category_of(file_name)
            .and_then(|c| self.category_dirs.get(c))
            .cloned()
            .unwrap_or_else(|| self.effective_download_dir())
    }

    /// The folder used when a download names none.
    pub fn effective_download_dir(&self) -> PathBuf {
        if let Some(d) = &self.download_dir {
            return d.clone();
        }
        let home = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Downloads"));
        match home {
            Some(d) if d.is_dir() => d,
            _ => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_documented_table() {
        let d = Settings::default();
        for info in SETTINGS {
            let mut s = Settings::default();
            s.apply(info.key, info.default).unwrap();
            assert_eq!(s, d, "default of {} must parse to the default", info.key);
        }
    }

    #[test]
    fn validation() {
        let mut s = Settings::default();
        assert!(s.apply("segments", "999").is_err());
        assert!(s.apply("segments", "0").is_err());
        assert!(s.apply("max_speed", "fast").is_err());
        assert!(s.apply("window", "25:99-01:00").is_err());
        assert!(s.apply("nope", "1").is_err());
        s.apply("segments", "16").unwrap();
        s.apply("max_speed", "2M").unwrap();
        s.apply("window", "01:00-06:30").unwrap();
        assert_eq!(
            (s.segments, s.max_speed, s.window),
            (16, 2 << 20, Some((60, 390)))
        );
    }

    #[test]
    fn category_folders_and_proxy() {
        let mut s = Settings::default();
        s.apply("download_dir", "/data/dl").unwrap();
        s.apply("dir_video", "/data/video").unwrap();
        assert_eq!(s.folder_for("clip.MKV"), PathBuf::from("/data/video"));
        assert_eq!(
            s.folder_for("song.mp3"),
            PathBuf::from("/data/dl"),
            "no music folder set: default folder"
        );
        assert_eq!(s.folder_for("noext"), PathBuf::from("/data/dl"));
        assert!(
            s.apply("dir_nonsense", "/x").is_err(),
            "only real categories"
        );
        s.apply("dir_video", "").unwrap();
        assert_eq!(s.folder_for("clip.mkv"), PathBuf::from("/data/dl"));
        for ok in [
            "",
            "none",
            "http://proxy:3128",
            "socks5://127.0.0.1:1080",
            "https://user:pw@p.example:443",
        ] {
            assert!(s.apply("proxy", ok).is_ok(), "{ok}");
        }
        for bad in ["proxy:3128", "ftp://p:21", "http://", "nonsense"] {
            assert!(s.apply("proxy", bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn every_category_has_a_folder_setting() {
        for c in crate::categories::all() {
            let key = format!("dir_{}", c.id);
            assert!(
                SETTINGS.iter().any(|i| i.key == key),
                "missing setting {key}"
            );
        }
    }

    #[test]
    fn persistence_and_reset() {
        let st = Store::open_in_memory().unwrap();
        Settings::set(&st, "segments", "3").unwrap();
        Settings::set(&st, "window", "22:00-06:00").unwrap();
        assert!(Settings::set(&st, "segments", "x").is_err());
        let s = Settings::load(&st);
        assert_eq!((s.segments, s.window), (3, Some((1320, 360))));
        Settings::set(&st, "segments", "").unwrap();
        assert_eq!(Settings::load(&st).segments, 8);
    }
}
