# Cawget

**Cawget Download Manager**: a fast segmented download manager for Linux (Rust, Tauri). It comes as a command
line tool (`cawget-cli`) and a desktop app (Cawget) with a Svelte interface.

Cawget is an independent implementation. It uses no third-party proprietary code or assets.

![Cawget main window on a wide screen](docs/screenshots/wide.png)

> **Status:** 0.1.1, an early release. Expect rough edges; see [Planned](#planned).

## Install

Download a package from the [Releases page](https://github.com/mahziyarng98/cawget/releases):

- **AppImage** (any x86_64 Linux, no installation): `chmod +x Cawget_<version>_amd64.AppImage`, then run it.
- **Debian / Ubuntu:** `sudo apt install ./Cawget_<version>_amd64.deb`, then start **Cawget** from the menu.
- **Fedora / openSUSE:** `sudo dnf install ./Cawget-<version>-1.x86_64.rpm` (or `zypper install`).
- **Flatpak bundle** (x86_64, *not on Flathub*): download `cawget-0.1.1-x86_64.flatpak`, then run
  `flatpak install --user cawget-0.1.1-x86_64.flatpak` ([details](docs/INSTALL.md#flatpak-bundle)).
- **Arch Linux:** an AUR package is *coming soon* (AUR registration is temporarily closed). For now, download the
  [PKGBUILD](https://raw.githubusercontent.com/mahziyarng98/cawget/main/packaging/aur/PKGBUILD) into an empty
  folder and run `makepkg -si` there ([details](docs/INSTALL.md#arch-linux)), or use the AppImage.

Packages are built and the automated tests run on Ubuntu 22.04 (x86_64). The AppImage was manually tested on Arch
Linux (Hyprland) on 2026-11-04; installing the .deb, the .rpm and the Arch package (`makepkg -si`) is untested so
far. Step-by-step instructions (checksums, removing, where your data is): [docs/INSTALL.md](docs/INSTALL.md).

## Features

- **Parallel segments:** a file is split into byte ranges fetched over several HTTP/HTTPS connections and written
  straight into place; an idle connection takes over the tail of a slow one.
- **Resume:** progress is checkpointed every second. After Ctrl-C, a crash or `kill -9` the download continues where
  it stopped; server validators (`ETag` / `Last-Modified`) are re-checked, so a changed file restarts cleanly.
- **Queue and scheduler:** persistent queue (SQLite), concurrent-job limit, start-at time, daily start window,
  retries with backoff, global speed limit, SHA-256 verification.
- **Categories:** files are sorted by type, with one download folder per category.
- **Network options:** proxy setting, custom CA file, custom headers, referer and cookies. Tested end to end: an
  `http://` proxy for `http://` downloads, and `none` to bypass it. Accepted but untested: proxies taken from the
  environment, `https://` downloads through a proxy, `https://` proxy URLs and `socks5://` proxies.
- **Keyring credentials:** logins for downloads and proxies are stored in the system keyring (GNOME Keyring/
  KWallet via Secret Service), not in the database; see [Credentials and the system keyring](#credentials-and-the-system-keyring).
- **Desktop app:** sortable and reorderable table, per-download progress window with a segment map, light and dark
  themes, right-to-left layout, keyboard shortcuts (F1 lists them), system tray (needs a desktop that shows
  StatusNotifier icons; GNOME needs an extension, see [docs/INSTALL.md](docs/INSTALL.md#troubleshooting)).
- **Any window size:** from 480x360 up to 4K, including half-screen tiles: the toolbar drops labels and then moves
  less-used buttons into a "…" (more actions) menu, the menus fold into one menu button, the categories become an
  icon rail (or a drawer) with a resizable splitter, low-priority columns hide and narrow windows get two-line rows.
  Zoom with Ctrl+= / Ctrl+- / Ctrl+0; the window size is remembered.
- **CLI:** `get`, `add`, `run`, `list`, `status`, `pause`, `resume`, `rm`, `clean`, `config`. See `man ./packaging/cawget-cli.1`.

| Half-screen tile (light theme) | Compact window |
|---|---|
| ![Cawget in a half-screen tile](docs/screenshots/half-screen.png) | ![Cawget in a small window](docs/screenshots/compact.png) |

## Planned

Not available yet. Some of these are visible in the app but disabled (marked "Coming soon"):

- Bulk add and page scanner
- Import / export of download lists
- Named queues with their own start/stop
- Saved per-site logins
- Start on login, clipboard watching
- Taking over downloads from web browsers (extension)
- Protocols other than HTTP and HTTPS (no FTP, torrent or metalink)
- More packages: the AUR package `cawget-bin`, a Flathub listing, Snap

Known limit: a file that changes on the server *during* a run, without its size changing, is not detected
(changes between runs are).

## Build

To install a release package instead (AppImage, .deb, .rpm), see [docs/INSTALL.md](docs/INSTALL.md).

Requirements: Rust 1.90 or newer, a C compiler, and Node.js `^20.19.0 || ^22.12.0 || >=24.0.0` (what Vite 8, its
Svelte plugin and Vitest accept together). For the desktop app also the Tauri Linux prerequisites
(`webkit2gtk-4.1`, `gtk3`, `librsvg`, ...).

```sh
# CLI only
cargo build --release -p cawget-cli           # target/release/cawget-cli

# tests (the GUI crate is built separately, below)
cargo test --workspace --exclude cawget

# desktop app
cd gui
npm ci
npm run check && npm test
npx tauri build                               # target/release/bundle/: AppImage, .deb and .rpm
```

Notes for `npx tauri build`:

- Run it from `gui/`; Tauri runs `npm run build` for the frontend first.
- If the AppImage step fails inside `strip`, build with `NO_STRIP=true`. The release profile already strips the
  binaries.
- To keep your home directory and checkout path out of the binaries, remap them (run from `gui/`). This is the exact
  command used for the release build:

  ```sh
  RUSTFLAGS="--remap-path-prefix=$HOME=~ --remap-path-prefix=$(cd .. && pwd)=." NO_STRIP=true npx tauri build
  ```

  When several prefixes match, the later flag wins, so the checkout (often inside `$HOME`) is listed last.
- `npx tauri dev` starts the app with the Vite dev server on `127.0.0.1:1420`.

## Data directory and migration

Cawget keeps its queue and settings in one SQLite database:

```
~/.local/share/cawget/cawget.db          ($XDG_DATA_HOME/cawget/cawget.db)
```

Override it with `CAWGET_DB`, or in the CLI with `--db FILE`.

Pre-release builds of this project used other default locations. On first start, if `cawget.db` does not exist,
the first of these that exists is **copied** to the new location (main file and write-ahead log; the copy is
integrity-checked before it is used):

1. `~/.local/share/jackdaw/jackdaw.db`
2. `~/.local/share/segget/segget.db`

The old file is never modified, moved or deleted. If another process is still using it, it is used in place for
that run and copied on a later start. The variables `JACKDAW_DB` and `SEGGET_DB` still work as deprecated aliases
of `CAWGET_DB` (in that order of precedence); a note is printed when one is used.

## Credentials and the system keyring

Logins entered for a download, proxy URLs of the form `http://user:password@host:port`, `--cookie` and `-H`
values are stored in your **system keyring** (GNOME Keyring or KWallet, via the Secret Service), **never** in the
queue database. The database keeps only a reference per login; when a download starts, its credentials are read
back from the keyring (which must be unlocked).

If no keyring is available (no Secret Service daemon is running, or a sandbox without permission), Cawget does
**not** fall back to plaintext: saving logins fails with a clear error and downloads that need them do not start.
Databases from older versions that still contain plaintext are migrated on the first start after the upgrade:

1. the database is backed up next to it (`cawget.db.pre-keyring-<timestamp>.bak`, mode `0600`; it contains the
   old plaintext — delete it once you trust the keyring copy);
2. each login is written to the keyring and verified by reading it back;
3. only after verification is the plaintext cleared from the database.

If the keyring is unavailable, nothing is migrated, the plaintext stays, a warning is logged, and the migration is
retried on the next start. The migration is idempotent and safe to interrupt.

Directories Cawget creates for the database are `0700`, and the database files it creates (`cawget.db`, its
`-wal`/`-shm` files, its `.lock` file, and a copy made by the migration) are `0600`, so other local users cannot
read them. Cawget does not change the permissions of files or directories that already exist, for example a
database you point it at with `CAWGET_DB`/`--db`, or the legacy databases it copies from; check those yourself.
Root, and anything running as your user, can still read the keyring while your session is unlocked. Do not share
the database or back it up to untrusted places, and prefer short-lived credentials.

## Layout

```
crates/cawget-core         engine library: segments, store, job runner, manager/scheduler, limiter
crates/cawget-cli          `cawget-cli` command
crates/cawget-testserver   in-process HTTP/HTTPS server used by the tests
gui/                       desktop app: Svelte 5 frontend (src), Tauri v2 shell (src-tauri)
packaging/                 desktop entry, man page
shared/                    category table shared by the engine and the UI
```

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE) and [NOTICE](NOTICE). Contributions: [CONTRIBUTING.md](CONTRIBUTING.md).
Security reports: [SECURITY.md](SECURITY.md).

Third-party components compiled into the programs (Rust crates and npm packages) and their licence texts are
listed in [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md), generated by `scripts/third_party_licenses.py`. The
AppImage also bundles system libraries (GTK, WebKitGTK and their dependencies), listed with their licences in
[docs/BUNDLED_LIBRARIES.md](docs/BUNDLED_LIBRARIES.md); both files are included inside the AppImage, and each
release has the exact bundled list attached.

### Source code of bundled LGPL/GPL libraries

The LGPL and GPL libraries in the AppImage are unmodified builds from the distribution of the build machine
(Ubuntu 22.04 for release builds). Their complete source is the corresponding source package:
`apt-get source <package>` on Ubuntu 22.04, or https://launchpad.net/ubuntu/+source/<package>. The package
names and versions of each release are in its attached `bundled-libraries.txt`. Upstream sources are linked in
[docs/BUNDLED_LIBRARIES.md](docs/BUNDLED_LIBRARIES.md). The .deb and .rpm packages bundle no system
libraries.
