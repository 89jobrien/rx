#!/usr/bin/env bash

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
DEMO_HOME=$(mktemp -d)
trap 'rm -rf "$DEMO_HOME"' EXIT

export XDG_CONFIG_HOME="$DEMO_HOME/config"
export RUSTC_WRAPPER=""

RX_BIN=(cargo run --quiet -p rx-install --bin rx --)
RXX_BIN=(cargo run --quiet -p rxx --bin rxx --)
EXAMPLE_DIR="$ROOT/examples/scripts"
PREFLIGHT_SHELL="$ROOT/scripts/preflight.sh"
PREFLIGHT_RUST="$ROOT/.ctx/scripts/preflight.rs"
REGISTRY_PATH="$XDG_CONFIG_HOME/rx/registry.json"

run() {
  printf '\n$ %s\n' "$*"
  "$@"
}

for tool in rust-script uv bun bash zsh fish nu; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "$tool is required for the smoke test but is not on PATH." >&2
    exit 1
  fi
done

echo "rx smoke root: $ROOT"
echo "temporary XDG_CONFIG_HOME: $XDG_CONFIG_HOME"

run "${RX_BIN[@]}" install "$PREFLIGHT_SHELL"
run "${RX_BIN[@]}" install "$EXAMPLE_DIR"
run "${RX_BIN[@]}" list
run cat "$REGISTRY_PATH"
run "${RX_BIN[@]}" run preflight -- "$PREFLIGHT_SHELL" "$PREFLIGHT_RUST"
run "${RXX_BIN[@]}" "$PREFLIGHT_RUST"
run "${RX_BIN[@]}" run hello-rust -- --name rx
run "${RX_BIN[@]}" run hello-python -- --name rx
run "${RXX_BIN[@]}" "$EXAMPLE_DIR/hello-javascript.js" -- --name direct
run "${RXX_BIN[@]}" "$EXAMPLE_DIR/hello-typescript.ts" -- --name direct
run "${RXX_BIN[@]}" "$EXAMPLE_DIR/hello-bash.sh" -- --name direct
run "${RXX_BIN[@]}" "$EXAMPLE_DIR/hello-zsh.zsh" -- --name direct
run "${RXX_BIN[@]}" "$EXAMPLE_DIR/hello-fish.fish" -- --name direct
run "${RXX_BIN[@]}" "$EXAMPLE_DIR/hello-nushell.nu" -- --name direct
