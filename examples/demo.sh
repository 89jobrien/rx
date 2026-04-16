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
PREFLIGHT_RUST="$ROOT/scripts/preflight.rs"
REGISTRY_PATH="$XDG_CONFIG_HOME/rx/registry.json"

require_tool() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "$1 is required for this demo but is not on PATH." >&2
    exit 1
  fi
}

section() {
  printf '\n== %s ==\n' "$1"
}

run() {
  printf '\n$ %s\n' "$*"
  "$@"
}

for tool in rust-script uv bun bash zsh fish nu; do
  require_tool "$tool"
done

cat <<EOF
rx demo
repo root: $ROOT
temporary XDG_CONFIG_HOME: $XDG_CONFIG_HOME

This walkthrough installs the shell preflight as the fast default command,
keeps the richer Rust preflight alongside it for direct execution, shows the
generated registry, and then exercises each runtime through rxx.
EOF

section "1. Install the shell preflight script"
run "${RX_BIN[@]}" install "$PREFLIGHT_SHELL"

section "2. Install the example script directory"
run "${RX_BIN[@]}" install "$EXAMPLE_DIR"

section "3. Discover what rx installed"
run "${RX_BIN[@]}" list

section "4. Inspect the registry entry"
run cat "$REGISTRY_PATH"

section "5. Execute installed commands with rx run"
run "${RX_BIN[@]}" run preflight
run "${RX_BIN[@]}" run hello-rust -- --name rx
run "${RX_BIN[@]}" run hello-python -- --name rx

section "6. Compare direct preflight scripts with rxx"
run "${RXX_BIN[@]}" "$PREFLIGHT_SHELL"
run "${RXX_BIN[@]}" "$PREFLIGHT_RUST"

section "7. Execute source files directly with rxx"
run "${RXX_BIN[@]}" "$EXAMPLE_DIR/hello-javascript.js" -- --name direct
run "${RXX_BIN[@]}" "$EXAMPLE_DIR/hello-typescript.ts" -- --name direct
run "${RXX_BIN[@]}" "$EXAMPLE_DIR/hello-bash.sh" -- --name direct
run "${RXX_BIN[@]}" "$EXAMPLE_DIR/hello-zsh.zsh" -- --name direct
run "${RXX_BIN[@]}" "$EXAMPLE_DIR/hello-fish.fish" -- --name direct
run "${RXX_BIN[@]}" "$EXAMPLE_DIR/hello-nushell.nu" -- --name direct

section "Done"
cat <<EOF
The demo used a temporary config root and cleaned it up on exit.
If you want a terse verification pass instead, run:

  ./examples/smoke.sh
EOF
