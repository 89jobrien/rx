# Repository Guidelines

## Project Structure & Module Organization

This repo is a small Rust workspace. Core execution planning lives in `crates/rx-core`, JSON-backed
registry and fetch logic lives in `crates/rx-registry-json`, the `rx` CLI is in
`crates/rx-install`, and the direct-run `rxx` CLI is in `crates/rxx`. Example scripts and
walkthroughs live under `scripts/` and `examples/`. CI workflows are in `.github/workflows/`.
Handoff state is tracked in `.ctx/`, but only durable handoff YAML should be committed.

## Build, Test, and Development Commands

- `mise install`: bootstrap the pinned local toolchain.
- `cargo check --workspace`: fast workspace compile check.
- `cargo fmt --all -- --check`: enforce formatting.
- `cargo clippy --all-targets --workspace -- -D warnings`: lint with warnings treated as errors.
- `cargo test --workspace`: run unit tests.
- `cargo build --locked --workspace`: match the CI build gate.
- `cargo run --quiet -p rx-install --bin rx -- list`: run the main CLI locally.
- `./examples/demo.sh` or `./examples/smoke.sh`: exercise install/run flows in a temporary XDG root.

## Coding Style & Naming Conventions

Use Rust 2024 edition defaults and keep code `rustfmt`-clean. Prefer small, focused functions and
clear error contexts with `anyhow` or `Context` when crossing I/O boundaries. Follow existing crate
naming and responsibility boundaries instead of adding cross-cutting logic to `main.rs`. Use
descriptive snake_case for functions and variables, and keep command-facing names aligned with the
installed binary or script stem, such as `preflight`.

## Testing Guidelines

Tests currently live inline under `#[cfg(test)]` modules in `crates/rx-core/src/lib.rs` and
`crates/rx-install/src/main.rs`. Add tests alongside the code they cover. Prefer behavior-focused
test names such as `execute_plan_learns_successful_candidate_prefix`. Before opening a PR, run the
same four gates CI runs: `fmt`, `clippy`, `test`, and `build`.

## Commit & Pull Request Guidelines

Recent history uses short conventional-style subjects such as `feat: ...`, `refactor: ...`, and
`docs: ...`. Keep commits scoped and imperative. PRs should explain the user-visible change,
mention any config or behavior changes, and link the relevant issue or handoff item when one
exists. Include CLI examples when the change affects command resolution, prefixes, or install/run
behavior.

## Configuration Notes

User-specific behavior comes from `~/.config/rx/`, especially `prefixes.toml`. Do not hardcode
personal machine state into tracked files. When editing docs or examples, preserve the current XDG
and passthrough-command model described in `README.md`.
