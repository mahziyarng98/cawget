# Flatpak packaging for Cawget

Phase B draft. See `FLATPAK_AUDIT.md`
(`/tmp/cawget-flatpak-audit-report/FLATPAK_AUDIT.md`) for the full audit and
the rationale for the ID, finish-args and module split.

## Layout

| Path | Purpose |
|---|---|
| `io.github.mahziyarng98.cawget.yml` | Main manifest (name = app ID, for `flatpak-builder-lint manifest`). Runtime `org.gnome.Platform//51`, SDK extensions `rust-stable`/`node24` (no branch suffix — flatpak-builder matches them against the Freedesktop SDK branch, 26.08 for GNOME 51). |
| `build-local.sh` | **Main local path**: creates a venv in `~/.cache/cawget-flatpak/work` (reused between runs), clones `flatpak-builder-tools` (prints its commit), installs generator deps, regenerates both source files and builds with `flatpak-builder --user`. Run `bash packaging/flatpak/build-local.sh`. |
| `io.github.mahziyarng98.cawget.metainfo.xml` | Flatpak copy of `packaging/io.github.mahziyarng98.cawget.metainfo.xml`, identical except `<launchable>` is `io.github.mahziyarng98.cawget.desktop` (the Flatpak renames the desktop file; the .deb/AppImage metainfo keeps `Cawget.desktop`). **Keep the two in sync** when editing. It is a local `type: file` source of the manifest, so no `sed` patching is needed. |
| `cargo-sources.json` | **Generated**: vendored crate sources from the workspace-root `Cargo.lock` (covers `gui/src-tauri`'s crate `cawget`, a workspace member). |
| `node-sources.json` | **Generated**: npm registry sources from `gui/package-lock.json`. |
| `shared-modules/` | Local copies of `flathub/shared-modules` files used by the manifest (`libayatana-appindicator-gtk3.json` + its patches, `intltool-0.51.json` + its patch). **Before a Flathub submission, replace this directory with a real git submodule** (see below). |

## Runtime / extensions (verified on the build machine)

- `org.gnome.Platform//51` + `org.gnome.Sdk//51` installed; GNOME 51 is built
  on Freedesktop 26.08.
- `org.freedesktop.Sdk.Extension.rust-stable//26.08` and
  `org.freedesktop.Sdk.Extension.node24//26.08` installed.
- `libwebkit2gtk-4.1.so.0` present in the runtime.
- `sdk-extensions` entries take **no branch suffix**: flatpak-builder matches
  them against the `org.freedesktop.Sdk` branch of the selected SDK (the
  flatpak-builder-tools README examples list them without a branch).

## Why two build modules + a shared module

- `libayatana-appindicator` is required at runtime: the tray icon goes through
  the `tray-icon` crate → `libappindicator` → `libloading` (`Cargo.lock`),
  which dlopens the system library. GNOME runtime does not provide it, so it
  is built from the Flathub shared-modules recipe.
- `gui` builds the Svelte/Vite frontend with npm fully offline (sources from
  `node-sources.json`). Dev dependencies are needed (vite, svelte), so the
  generator must **not** be run with `--no-devel`.
- `cawget` builds the Rust workspace and embeds the frontend built by `gui`
  (Tauri `custom-protocol` feature; `frontendDist ../dist`). The `gui` module
  installs its `dist` to `${FLATPAK_DEST}/lib/cawget/gui-dist`, and the
  `cawget` module copies it to `gui/dist` before `cargo build`.

---

## (a) Regenerate `cargo-sources.json` and `node-sources.json`

The simplest path: **`bash packaging/flatpak/build-local.sh`** — it does
steps (a) and (c) below in one go (venv outside the repo, fresh clone of
`flatpak-builder-tools`, generator deps, both source files, then the build).

Manual alternative (same tools, same files):

Requirements: Python 3.10+ with `aiohttp >=3.9.5`, `PyYAML >=6.0.2`,
`tomlkit >=0.13.3` (cargo generator), plus the `flatpak-node-generator`
package from the `node/` subdirectory (node generator). This is exactly the
tooling Flathub uses for dependency manifests
(`github.com/flatpak/flatpak-builder-tools`).

From the repo root, with **no `gui/node_modules` present** (the node generator
requires an empty `node_modules`):

```sh
# cargo (generates the same file the manifest points at)
python3 /path/to/flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json

# node — npm lockfile in gui/
flatpak-node-generator npm gui/package-lock.json -o packaging/flatpak/node-sources.json
# Optional, only if a native module ever needs node headers:
#   ... --node-sdk-extension org.freedesktop.Sdk.Extension.node24//26.08
```

Never edit the generated files by hand; regenerate them for every release
(pin the new tag + commit in the manifest first). Commit both files.

## (b) Install flatpak + flatpak-builder

Debian/Ubuntu:

```sh
sudo apt install flatpak flatpak-builder
flatpak remote-add --if-not-exists --user flathub https://dl.flathub.org/repo/flathub.flatpakrepo
```

Arch (e.g. `flatpak` + `flatpak-builder` from extra; or AUR `flatpak-builder-git`):

```sh
sudo pacman -S flatpak flatpak-builder
flatpak remote-add --if-not-exists --user flathub https://dl.flathub.org/repo/flathub.flatpakrepo
```

Fedora:

```sh
sudo dnf install flatpak flatpak-builder
flatpak remote-add --if-not-exists --user flathub https://dl.flathub.org/repo/flathub.flatpakrepo
```

Install the runtime, SDK and SDK extensions (branch numbers match the
manifest's `runtime-version: '51'` and its Freedesktop SDK 26.08):

```sh
flatpak install --user flathub \
  org.gnome.Platform//51 \
  org.gnome.Sdk//51 \
  org.freedesktop.Sdk.Extension.rust-stable//26.08 \
  org.freedesktop.Sdk.Extension.node24//26.08
```

Pre-flight (already done on the build machine): WebKitGTK 4.1 is present in
the runtime (`libwebkit2gtk-4.1.so.0`).

## (c) Build and run locally

Main path (generates sources too):

```sh
bash packaging/flatpak/build-local.sh
```

Equivalently, if the generated `cargo-sources.json` / `node-sources.json`
already exist:

```sh
cd packaging/flatpak
flatpak-builder --user --install --force-clean \
  --state-dir=~/.cache/cawget-flatpak/state \
  ~/.cache/cawget-flatpak/builddir \
  io.github.mahziyarng98.cawget.yml
flatpak run io.github.mahziyarng98.cawget
```

Useful during iteration (add ccache):

```sh
flatpak-builder --user --install --force-clean --ccache \
  --state-dir=~/.cache/cawget-flatpak/state \
  ~/.cache/cawget-flatpak/builddir \
  io.github.mahziyarng98.cawget.yml
```

After `--install`, the sandbox behaviour can be inspected (audit §3): run with
session-bus logging to confirm the tray talk-name, and test the portal-based
file/folder chooser and "open file/folder" actions:

```sh
flatpak run --log-session-bus io.github.mahziyarng98.cawget
```

## (d) Lint (run after `build-local.sh`)

```sh
cd packaging/flatpak
flatpak-builder-lint manifest io.github.mahziyarng98.cawget.yml
# after a successful build (without sudo: flatpak install --user flathub org.flatpak.Builder,
# then prefix with `flatpak run --command=flatpak-builder-lint org.flatpak.Builder`):
flatpak-builder-lint builddir ~/.cache/cawget-flatpak/builddir
appstreamcli validate --pedantic --no-net io.github.mahziyarng98.cawget.metainfo.xml
desktop-file-validate ../cawget.desktop
```

`flatpak-builder-lint` checks both `manifest` and `repo`; the Flathub
pipeline also runs appstream validation and treats warnings as fatal
(docs.flathub.org/docs/for-app-authors/linter).

## Before submitting to Flathub (phase B+)

1. Replace `shared-modules/` with a real submodule and reference it:
   ```sh
   git submodule add https://github.com/flathub/shared-modules.git
   ```
   (keep the manifest's `shared-modules/libayatana-appindicator/...` path).
2. Regenerate `cargo-sources.json` / `node-sources.json` from the release tag.
3. At submission time, confirm the latest hosted GNOME runtime branch and
   update `runtime-version` (and the extension branches if they changed).
4. Confirm the two UNVERIFIED portal behaviours (rfd dialog, opener) and the
   tray D-Bus talk-name in the sandbox (audit §3), and the `xdg-download`
   download flow.
5. Decide the data-migration story (audit §4) and document it.
6. Open a PR against `flathub/flathub` `new-pr` with the manifest, generated
   sources and a permission justification for every `finish-args` entry
   (audit §6).

## Known deviations / caveats

- Screenshots `wide.png` (1440×900) and `half-screen.png` (940×1000) exceed
  the recommended ≤1000×700 window size (quality guidelines). They now point
  at the `v0.1.0` tag; re-shooting them is a phase-2 task.
- The `gui` module leaves its frontend copy at
  `${FLATPAK_DEST}/lib/cawget/gui-dist` inside the app (it is also embedded in
  the binary); acceptable for phase A, can be cleaned later.
- The app stores credentials in the system keyring (Secret Service). The
  manifest requests `--talk-name=org.freedesktop.secrets`; without it the
  sandbox cannot use the keyring and Cawget refuses to store new logins.
- The plaintext→keyring migration runs on the first start after the upgrade and
  backs the old database up next to it. See the "Credentials" section of
  README.md.
