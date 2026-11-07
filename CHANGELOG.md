# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added
- Credentials (download logins, proxy logins, cookies, custom headers) are stored in the system keyring
  (Secret Service: GNOME Keyring / KWallet), not in the SQLite database, which keeps only references. Without a
  keyring, saving logins fails with a clear error (no plaintext fallback) and downloads that need them do not
  start.
- On the first start after the upgrade, plaintext credentials in existing databases are migrated to the keyring:
  the database is backed up next to it (`cawget.db.pre-keyring-<timestamp>.bak`), each login is written and
  verified by reading it back, and only then is the plaintext cleared. If the keyring is unavailable the
  migration is postponed (the plaintext stays) and retried on the next start.
- Flatpak packaging requests Secret Service access (`--talk-name=org.freedesktop.secrets`).

## [0.1.0] - first public release

First public release of Cawget (Cawget Download Manager).

### Added
- Segmented HTTP/HTTPS downloads over parallel connections, with work stealing from slow segments.
- Resume after Ctrl-C, crash or `kill -9`; server validators (`ETag`, `Last-Modified`) are re-checked.
- Persistent queue in SQLite, concurrent-job limit, retries with backoff, global speed limit,
  SHA-256 verification, per-download start time and a daily start window.
- `cawget-cli` command line front end and the Cawget desktop app (Tauri v2, Svelte 5).
- Categories by file type (Archives, Documents, Audio, Software, Video) with one download folder per category, proxy
  settings, light/dark theme, RTL layout.
- Packages: AppImage, .deb and .rpm (with AppStream metadata, icons and licence files), and an AUR
  `cawget-bin` PKGBUILD. Third-party licences in THIRD_PARTY_LICENSES.md and docs/BUNDLED_LIBRARIES.md.
- Continuous integration and a release workflow that builds draft releases with checksums.
- Responsive window layout from 480x360 to 4K (toolbar overflow menu, folded menu bar, category rail/drawer with a
  resizable splitter, column priorities, compact two-line rows, dialogs that fit small windows), View > Columns,
  zoom (Ctrl+= / Ctrl+- / Ctrl+0) and a remembered window size.
- The data directory and database are created private to the user (`0700` / `0600`), since the queue can hold
  credentials in plain text.

### Notes for users of pre-release builds
- The data directory is `~/.local/share/cawget/` and the database variable is `CAWGET_DB`.
- On first start, an existing pre-release database is copied (never moved or changed), the first found of:
  `~/.local/share/jackdaw/jackdaw.db`, then `~/.local/share/segget/segget.db`.
- `JACKDAW_DB` and `SEGGET_DB` still work as deprecated aliases of `CAWGET_DB`, with a note.
- The default User-Agent is `Cawget/<version>`.
- Licence: GPL-3.0-or-later.
