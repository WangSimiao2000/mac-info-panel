#!/bin/zsh
set -euo pipefail

ROOT="${0:A:h:h}"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  source "$HOME/.cargo/env"
fi

if ! command -v cargo >/dev/null 2>&1; then
  print -u2 "Cargo is required. Install Rust with rustup first."
  exit 1
fi

cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

cargo build \
  --manifest-path plugins/example-status/Cargo.toml \
  --target wasm32-unknown-unknown \
  --release

PLUGIN_WASM="$ROOT/plugins/example-status/target/wasm32-unknown-unknown/release/mac_info_panel_plugin_example_status.wasm"
MIP_EXAMPLE_PLUGIN_WASM="$PLUGIN_WASM" \
  cargo test -p panel-plugin-host --test real_extism_plugin -- --ignored
