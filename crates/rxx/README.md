# rxx

`rxx` executes one compatible local script directly, without first copying it into the `rx`
install directory or writing a registry entry. The Cargo package is named `rx-rxx`; it builds the
`rxx` binary.

`rxx` reads the script, asks `rx-core` to detect its runtime and build an execution plan, then
spawns that plan with inherited stdin, stdout, and stderr. It does not write rx configuration, but
the executed script can still have its own side effects.

## Installation and Local Use

```bash
cargo install --path crates/rxx
cargo run --quiet -p rx-rxx --bin rxx -- ./scripts/preflight.sh
```

There are no feature flags or configuration files for this package.

## Command Line

```text
rxx <SCRIPT> [ARGS]...
```

The script path is required. All remaining arguments are appended to the selected launcher's
argument vector. Hyphen-prefixed values are accepted; an explicit `--` separator is also useful in
shell examples:

```bash
rxx ./scripts/preflight.sh
rxx ./examples/scripts/hello-python.py -- --name direct
rxx ./examples/scripts/hello-typescript.ts --name direct
rxx ./examples/scripts/hello-rust.rs -- --name direct
```

If the file cannot be read or its first line is unsupported, `rxx` fails before spawning a child.
If spawning succeeds, `rxx` exits with the child's status code; termination without a numeric code
maps to status 1.

## Supported Runtimes

Detection is based on an exact first-line shebang. The selected launchers are:

| Script family | Accepted shebangs                                                  | Invocation             |
| ------------- | ------------------------------------------------------------------ | ---------------------- |
| Rust script   | `env rust-script`, `/usr/bin/rust-script`                          | `rust-script <script>` |
| Python        | `env python`, `env python3`, `/usr/bin/python`, `/usr/bin/python3` | `uv run <script>`      |
| JavaScript    | `env node`, `/usr/bin/node`, `env bun`, `/usr/bin/bun`             | `bun <script>`         |
| TypeScript    | Node/Bun shebang and TS-family extension                           | `bun <script>`         |
| Bash          | `env bash`, `/bin/bash`                                            | `bash <script>`        |
| Zsh           | `env zsh`, `/bin/zsh`                                              | `zsh <script>`         |
| Fish          | `env fish`, `/usr/bin/fish`                                        | `fish <script>`        |
| Nushell       | `env nu`, `/usr/bin/nu`                                            | `nu <script>`          |
| Ruby          | `env ruby`, `/usr/bin/ruby`                                        | `ruby <script>`        |

TypeScript is distinguished from JavaScript by extensions `ts`, `tsx`, `mts`, or `cts`. The
required launcher must already be installed on `PATH`; `rxx` does not install interpreters or try
alternative launchers.

## Workspace Role

```text
rx-core              runtime detection and direct-run planning
rx-registry-json     FsScriptReader adapter used to read the script
rx-install           the install, registry, and multi-repo rx CLI
rx-rxx               the rxx direct-run CLI (this package)
rx-runner            standalone process runner; not currently used by rxx
```

The direct-run path uses `DirectRunRequest`, `plan_direct_run`, and `ExecutionPlan` from `rx-core`,
plus `FsScriptReader` from `rx-registry-json`. Unlike `rx run`, it does not consult
`registry.json`, shell aliases, or `prefixes.toml`.

## Development and Testing

From the workspace root:

```bash
cargo check -p rx-rxx
cargo fmt --all -- --check
cargo clippy -p rx-rxx --all-targets -- -D warnings
cargo test -p rx-rxx
cargo run --quiet -p rx-rxx --bin rxx -- --help
```

CLI integration tests create isolated temporary scripts and verify that a compatible Bash script
runs successfully while a file without a supported shebang fails. Runtime planning itself is
covered by `rx-core` unit and property tests. The workspace's `examples/demo.sh` and
`examples/smoke.sh` provide broader manual coverage when the supported launchers are installed.
