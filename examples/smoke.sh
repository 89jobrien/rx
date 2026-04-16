#!/usr/bin/env bash

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
DEMO_HOME=$(mktemp -d)
trap 'rm -rf "$DEMO_HOME"' EXIT

export XDG_CONFIG_HOME="$DEMO_HOME/config"
export RUSTC_WRAPPER=""

RX_BIN=(cargo run --quiet --bin rx --)
RXX_BIN=(cargo run --quiet --bin rxx --)
EXAMPLE_SCRIPT="$ROOT/examples/hello.rs"
REGISTRY_PATH="$XDG_CONFIG_HOME/rx/registry.json"

run() {
  printf '\n$ %s\n' "$*"
  "$@"
}

if ! command -v rust-script >/dev/null 2>&1; then
  echo "rust-script is required for the smoke test but is not on PATH." >&2
  exit 1
fi

echo "rx smoke root: $ROOT"
echo "temporary XDG_CONFIG_HOME: $XDG_CONFIG_HOME"

run "${RX_BIN[@]}" install "$EXAMPLE_SCRIPT"
run "${RX_BIN[@]}" list
run cat "$REGISTRY_PATH"
run "${RX_BIN[@]}" run hello -- --name rx
run "${RXX_BIN[@]}" "$EXAMPLE_SCRIPT" -- --name direct
