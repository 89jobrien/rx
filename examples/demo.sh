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

section() {
  printf '\n== %s ==\n' "$1"
}

run() {
  printf '\n$ %s\n' "$*"
  "$@"
}

if ! command -v rust-script >/dev/null 2>&1; then
  echo "rust-script is required for the demo but is not on PATH." >&2
  exit 1
fi

cat <<EOF
rx demo
repo root: $ROOT
temporary XDG_CONFIG_HOME: $XDG_CONFIG_HOME

This walkthrough installs a sample script, shows the generated registry,
runs the installed command through rx, and then runs the same script
directly through rxx.
EOF

section "1. Install the sample script"
run "${RX_BIN[@]}" install "$EXAMPLE_SCRIPT"

section "2. Discover what rx installed"
run "${RX_BIN[@]}" list

section "3. Inspect the registry entry"
run cat "$REGISTRY_PATH"

section "4. Execute the installed command with rx run"
run "${RX_BIN[@]}" run hello -- --name rx

section "5. Execute the source file directly with rxx"
run "${RXX_BIN[@]}" "$EXAMPLE_SCRIPT" -- --name direct

section "Done"
cat <<EOF
The demo used a temporary config root and cleaned it up on exit.
If you want a terse verification pass instead, run:

  ./examples/smoke.sh
EOF
