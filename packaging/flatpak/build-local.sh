#!/usr/bin/env bash
# build-local.sh — regenerate the Flatpak dependency sources and build Cawget
# locally with flatpak-builder.
#
# This is the main local path (see README.md). It:
#   1. creates (or reuses) a Python venv in ~/.cache/cawget-flatpak/work,
#      OUTSIDE the repo
#   2. shallow-clones (or refreshes) flatpak-builder-tools in that directory
#      and prints the commit hash used
#   3. installs the generators' dependencies into the venv
#   4. generates cargo-sources.json (from the workspace-root Cargo.lock) and
#      node-sources.json (from gui/package-lock.json) into packaging/flatpak/
#   5. runs flatpak-builder (user install, forced clean, fixed state/build dirs)
#   6. prints how to run the app
#
# No sudo is used. Run from anywhere; the script locates the repo root itself.
#
# Requirements: python3 (3.10+), git, and flatpak-builder installed with the
# runtime/ SDK/ extensions from README.md section (b).
set -euo pipefail

# ---------------------------------------------------------------------------
# 0) Proxy normalisation. pip (without PySocks) and some tools choke on
#    socks:// proxies, and ALL_PROXY overrides http(s)_proxy. Drop ALL_PROXY
#    and rewrite socks* values of http(s)_proxy to http:// on the same
#    host:port (assumes the proxy port speaks HTTP too). No-op without proxies.
# ---------------------------------------------------------------------------
unset ALL_PROXY all_proxy
for _v in http_proxy https_proxy HTTP_PROXY HTTPS_PROXY; do
  _val="${!_v-}"
  case "$_val" in
    socks*://*) export "$_v=http://${_val#*://}" ;;
  esac
done
unset _v _val

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"
echo "Repo root: $REPO_ROOT"

# ---------------------------------------------------------------------------
# 1) Scratch space OUTSIDE the repo, persistent: venv + tools clone.
# ---------------------------------------------------------------------------
CACHE="$HOME/.cache/cawget-flatpak"
WORK="$CACHE/work"
mkdir -p "$WORK"
echo "Scratch dir (reused between runs): $WORK"

VENV="$WORK/venv"
[ -x "$VENV/bin/python" ] || python3 -m venv "$VENV"

# ---------------------------------------------------------------------------
# 2) Shallow clone of flatpak-builder-tools (reused; refreshed on each run).
# ---------------------------------------------------------------------------
TOOLS="$WORK/flatpak-builder-tools"
if [ -d "$TOOLS/.git" ]; then
  git -C "$TOOLS" fetch --depth 1 origin HEAD
  git -C "$TOOLS" reset --hard FETCH_HEAD
else
  rm -rf "$TOOLS"
  git clone --depth 1 https://github.com/flatpak/flatpak-builder-tools.git "$TOOLS"
fi
TOOLS_COMMIT="$(git -C "$TOOLS" rev-parse HEAD)"
echo "flatpak-builder-tools commit: $TOOLS_COMMIT"

# ---------------------------------------------------------------------------
# 3) Generator dependencies into the venv.
#    - cargo generator (flatpak-cargo-generator.py) is a standalone script;
#      its deps per cargo/pyproject.toml: aiohttp >=3.9.5, PyYAML >=6.0.2,
#      tomlkit >=0.13.3.
#    - node generator ships as a Python package in the node/ subdirectory with
#      a `flatpak-node-generator` console script (node/pyproject.toml), so we
#      install that package (this also pulls aiohttp/pyyaml).
# ---------------------------------------------------------------------------
"$VENV/bin/pip" install --quiet "aiohttp<4,>=3.9.5" "PyYAML<7,>=6.0.2" "tomlkit>=0.13.3,<1"
"$VENV/bin/pip" install --quiet "$TOOLS/node"

# ---------------------------------------------------------------------------
# 4) Generate dependency sources.
#    Cargo: single workspace Cargo.lock at the repo root. gui/src-tauri's
#    crate "cawget" is a workspace member, so its deps are already in this
#    lockfile. The manifest references both generated files next to itself in
#    packaging/flatpak/.
#    npm: gui/package-lock.json. flatpak-node-generator REQUIRES that
#    gui/node_modules does not exist when it runs (node/README.md) — fail
#    loudly instead of generating wrong sources.
#    NOTE: we intentionally do NOT pass --no-devel, because the frontend build
#    needs the devDependencies (vite, svelte, ...).
# ---------------------------------------------------------------------------
if [ -d "gui/node_modules" ]; then
  echo "error: gui/node_modules exists; flatpak-node-generator must run without it (see node/README.md). Remove/move it and re-run." >&2
  exit 1
fi

echo "Generating packaging/flatpak/cargo-sources.json from Cargo.lock ..."
"$VENV/bin/python" "$TOOLS/cargo/flatpak-cargo-generator.py" Cargo.lock -o packaging/flatpak/cargo-sources.json

echo "Generating packaging/flatpak/node-sources.json from gui/package-lock.json ..."
"$VENV/bin/flatpak-node-generator" npm gui/package-lock.json -o packaging/flatpak/node-sources.json

# ---------------------------------------------------------------------------
# 5) Build (flatpak-builder, user install, no sudo). State dir and build dir
#    are fixed paths under ~/.cache so rebuilds reuse caches.
# ---------------------------------------------------------------------------
flatpak-builder --user --install --force-clean \
  --state-dir="$CACHE/state" \
  "$CACHE/builddir" \
  packaging/flatpak/io.github.mahziyarng98.cawget.yml

# ---------------------------------------------------------------------------
# 6) How to run.
# ---------------------------------------------------------------------------
echo "Build finished. Run the app with:"
echo "  flatpak run io.github.mahziyarng98.cawget"
