# Contributing

Thanks for your interest! Bug reports, small fixes and focused pull requests are welcome.
For anything bigger, open an issue first so we can agree on the approach.

## Setup

See the build instructions in [README.md](README.md) for requirements. Before sending a pull request, run this
from the repository root (it works on a fresh clone):

```sh
# frontend first: the desktop crate embeds gui/dist, so it must exist before cargo builds the workspace
(cd gui && npm ci && npm run check && npm test && npm run build)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 scripts/third_party_licenses.py --check   # after changing dependencies: run it without --check
```

Use `cargo fmt --all` to fix formatting reported by the check.

## Guidelines

- Keep changes small and focused; add or update a test for behaviour changes.
- Match the style of the surrounding code (comments, naming, error handling).
- The product name lives in one place (`productName` in `gui/src-tauri/tauri.conf.json`); the UI imports it
  from `gui/src/lib/brand.ts`, the Tauri shell reads it from the config, and the engine's `product_name!` macro
  must match it. `npm run check` (`gui/scripts/check-brand.mjs`) enforces this and rejects old product names.
- Contributions are accepted under the project licence (GPL-3.0-or-later). Do not submit code or assets
  you do not have the right to contribute, or that are taken from proprietary software.
- Never commit credentials, personal paths or local databases.
