# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

`rx` is a small Rust workspace for script installation and execution. It installs compatible
scripts from local files, directories, or remote URLs into a local command directory, records
them in a JSON registry, and can execute them later by name. `rxx` uses the same runtime
selection logic to execute a compatible script directly without installing it first.

## Build Commands

```bash
cargo build --workspace                        # build all crates
cargo check -q --all-targets --workspace      # fast validation
cargo test --workspace                        # run the full test suite
cargo clippy --all-targets --workspace -- -D warnings
cargo fmt --all -- --check

./examples/smoke.sh                           # terse end-to-end verification
./examples/demo.sh                            # guided local walkthrough
```

Use the workspace root for all commands.

## Workspace Structure

Four crates live under `crates/`:

- `rx-core` -- domain logic for source resolution, shebang/runtime detection, install rules, and
  execution planning.
- `rx-registry-json` -- JSON registry persistence plus HTTP fetching adapters and XDG default path
  resolution.
- `rx-install` -- the `rx` CLI for install, list, and run.
- `rxx` -- direct-run CLI for executing one compatible script without installing it.

## Architecture

`rx-core` owns the behavior and exposes the main seams:

- `RegistryStore` -- persistence port for listing and upserting installed scripts
- `RemoteScriptFetcher` -- remote fetch port used for URL installs
- `ExecutionPlan` -- normalized launch plan used by both `rx` and `rxx`

Keep runtime rules, naming rules, and install semantics in `rx-core`. CLI crates should stay thin
and mostly parse args, call planning/install functions, and execute the returned plan.

## Key Invariants

- Script acceptance is shebang-based. If you change supported runtimes, update the validation logic
  and README examples together.
- Installing a directory skips incompatible files but fails if nothing compatible was found.
- Installed command names come from the filename stem, so `foo.sh` and `foo.rs` collide by design.
- GitHub `blob` URLs are normalized to `raw.githubusercontent.com` before fetch.
- Default install and registry paths are XDG-style (`~/.config/rx` or `$XDG_CONFIG_HOME/rx`).

## Working Notes

- Prefer changing behavior through tests in the crate that owns it rather than adding logic to the
  CLI entrypoints.
- `examples/` is the fastest way to verify real command flow without mutating your normal config,
  because the scripts isolate their own XDG root.
