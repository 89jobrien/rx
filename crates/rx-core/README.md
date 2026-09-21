# rx-core

`rx-core` contains the domain types and orchestration used by the `rx` and `rxx` command-line
tools. It detects script runtimes, builds execution plans, models registry ports, discovers and
filters repositories, collects git status, builds cross-repository Cargo graphs, and fans commands
out across repositories.

The crate includes filesystem and process adapters for the multi-repository features. Script
persistence, HTTP fetching, and script file I/O are implemented separately by
`rx-registry-json`.

## Workspace Role

```text
rx-core            domain types, ports, planning, and multi-repo operations
rx-registry-json   JSON, HTTP, and script-filesystem adapters for rx-core ports
rx-install         the rx CLI
rx-rxx             the rxx direct-run CLI
rx-runner          standalone polling process runner; not wired into rx or rxx
```

The script APIs use ports so callers can replace storage and I/O in tests or other front ends:

- `RegistryStore` lists and upserts installed commands.
- `RemoteScriptFetcher` retrieves a remote script as text.
- `ScriptReader` reads a script before direct execution.
- `ScriptWriter` writes a named script into an install directory.
- `DirectoryScanner` returns files beneath a source directory.

## Script APIs

The script types are defined in `rx_core::script` and re-exported at the crate root.

| API                     | Purpose                                                                  |
| ----------------------- | ------------------------------------------------------------------------ |
| `install`               | Resolve a file, directory, or HTTP(S) URL and install compatible scripts |
| `list_installed`        | Return entries supplied by a `RegistryStore`                             |
| `plan_installed_run`    | Resolve a registry name into an `ExecutionPlan`                          |
| `plan_direct_run`       | Detect a local script's runtime and build an `ExecutionPlan`             |
| `detect_runtime`        | Map a supported first-line shebang to a `Runtime`                        |
| `apply_command_prefix`  | Wrap a plan with a command such as `op plugin run --`                    |
| `format_registry_entry` | Produce the tab-delimited row used by `rx list`                          |

`install` derives the command name from the source filename stem. A directory source is scanned
through `DirectoryScanner`; compatible files are installed and incompatible files are reported in
`InstallReport::skipped`. A file or URL must itself be compatible. Registry upsert happens after the
source has been processed.

### Runtime Detection and Launchers

The first line must exactly match a supported shebang. The resulting execution plan uses these
launchers:

| Runtime     | Accepted shebang families                                          | Planned launcher |
| ----------- | ------------------------------------------------------------------ | ---------------- |
| Rust script | `#!/usr/bin/env rust-script`, `#!/usr/bin/rust-script`             | `rust-script`    |
| Python      | `env python`, `env python3`, `/usr/bin/python`, `/usr/bin/python3` | `uv run`         |
| JavaScript  | `env node`, `/usr/bin/node`, `env bun`, `/usr/bin/bun`             | `bun`            |
| TypeScript  | Node/Bun shebang plus a TS-family extension                        | `bun`            |
| Bash        | `#!/usr/bin/env bash`, `#!/bin/bash`                               | `bash`           |
| Zsh         | `#!/usr/bin/env zsh`, `#!/bin/zsh`                                 | `zsh`            |
| Fish        | `#!/usr/bin/env fish`, `#!/usr/bin/fish`                           | `fish`           |
| Nushell     | `#!/usr/bin/env nu`, `#!/usr/bin/nu`                               | `nu`             |
| Ruby        | `#!/usr/bin/env ruby`, `#!/usr/bin/ruby`                           | `ruby`           |

JavaScript and TypeScript share shebangs. Extensions `ts`, `tsx`, `mts`, and `cts` select
TypeScript; other labels select JavaScript. `rx-core` builds plans but does not verify that a
launcher is installed.

```rust
use rx_core::{DirectRunRequest, ExecutionPlan, ScriptReader, plan_direct_run};
use std::path::{Path, PathBuf};

struct Reader;

impl ScriptReader for Reader {
    fn read(&self, _path: &Path) -> anyhow::Result<String> {
        Ok("#!/usr/bin/env python3\nprint('hello')\n".to_string())
    }
}

let plan: ExecutionPlan = plan_direct_run(
    &DirectRunRequest {
        script_path: PathBuf::from("hello.py"),
        args: vec!["--verbose".to_string()],
    },
    &Reader,
)?;
assert_eq!(plan.program, "uv");
assert_eq!(plan.args, ["run", "hello.py", "--verbose"]);
# Ok::<(), anyhow::Error>(())
```

## Repository APIs

### Discovery and Filtering

`rx_core::repo` provides `Manifest`, `Defaults`, and `RepoMeta` for `repos.toml`, plus two
`RepoSource` adapters:

- `ManifestRepoSource` returns entries from an already loaded manifest.
- `ScanRepoSource` recursively discovers directories containing a `.git` file or directory.

`load_manifest` expands leading `~` paths and resolves relative repository paths against the
manifest directory. Duplicate repository names are rejected. `parse_filter` accepts `tag=value`,
`role=value`, `language=value`, `name=value`, and `name~glob`; the glob syntax supports only `*`.
Multiple `Filter` values passed to `apply_filters` are ANDed.

```toml
[defaults]
root = "~/dev"
ignore = ["target", "node_modules"]

[[repo]]
name = "rx"
path = "~/dev/rx"
role = "tool"
language = "rust"
tags = ["rust", "active"]
```

### Status

`rx_core::status` parses `git status --porcelain=v2 --branch` into `RepoStatus` values.
`collect_status` probes repositories in parallel through the `GitProbe` trait. The included
`GitCliProbe` also reads the latest commit with `git log -1`. Parsing helpers are public for custom
adapters and tests.

### Cargo Dependency Graph

`rx_core::graph` scans root packages and explicit or simple `directory/*` workspace members.
`build_graph` retains edges only when both package names are found in the scanned repositories.
`DepGraph` supports transitive `deps`, transitive `who_uses`, and dependency-first `topo_order`.
`render_graph` emits tree, JSON, or Mermaid text.

### Fan-Out Execution

`rx_core::fan::fan_out` executes one argument vector in each repository directory using a bounded
Rayon pool. `FanConfig` controls concurrency, an optional per-process timeout, and fail-fast
behavior. `FanReport` retains stdout, stderr, exit data, and aggregate counts; `render_grouped`
produces the CLI's human-readable report. Commands are passed as an argument vector, not through a
shell.

## Feature Flags

`test-support` exposes `rx_core::conformance` outside this crate's own tests and enables its
optional `tempfile` dependency. Adapter crates can use these reusable checks for `RegistryStore`,
`RemoteScriptFetcher`, `ScriptReader`, `ScriptWriter`, and `DirectoryScanner` implementations.

```toml
[dev-dependencies]
rx-core = { path = "../rx-core", features = ["test-support"] }
```

## Development

From the workspace root:

```bash
cargo check -p rx-core
cargo fmt --all -- --check
cargo clippy -p rx-core --all-targets -- -D warnings
cargo test -p rx-core
```

Tests cover script installation and planning, runtime serialization, port conformance, repository
manifest and discovery behavior, status parsing, dependency graph queries and rendering, fan-out
execution, and property-based cases for shebangs, URLs, and command-name derivation.
