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
cargo fmt --manifest-path vendor/piccolo/Cargo.toml -- --check
cargo fmt --manifest-path plugins/lua-runner/Cargo.toml -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo test --manifest-path plugins/lua-runner/Cargo.toml --lib
cargo test --manifest-path vendor/piccolo/Cargo.toml --test fuel_checked
cargo clippy --workspace --all-targets -- -D warnings

cargo build \
  --manifest-path plugins/example-status/Cargo.toml \
  --target wasm32-unknown-unknown \
  --release
cargo build \
  --manifest-path plugins/lua-runner/Cargo.toml \
  --target wasm32-unknown-unknown \
  --release
cargo clippy \
  --manifest-path plugins/lua-runner/Cargo.toml \
  --target wasm32-unknown-unknown \
  -- -D warnings

PLUGIN_WASM="$ROOT/plugins/example-status/target/wasm32-unknown-unknown/release/info_panel_plugin_example_status.wasm"
LUA_RUNNER_WASM="$ROOT/plugins/lua-runner/target/wasm32-unknown-unknown/release/info_panel_lua_runner.wasm"
INFO_PANEL_EXAMPLE_PLUGIN_WASM="$PLUGIN_WASM" \
  cargo test -p panel-plugin-host --test real_extism_plugin -- --ignored
INFO_PANEL_LUA_RUNNER_WASM="$LUA_RUNNER_WASM" \
  cargo test -p panel-plugin-runtime-extism --test lua_runner -- --ignored --test-threads=1
INFO_PANEL_LUA_RUNNER_WASM="$LUA_RUNNER_WASM" \
  cargo test -p panel-plugin-host --test real_lua_plugin -- --ignored --test-threads=1
