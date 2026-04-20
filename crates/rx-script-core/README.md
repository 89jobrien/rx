# rx-script-core

Core runtime detection and execution planning for the `rx` script installer workspace.

## What it does

`rx-script-core` owns all behavior that is shared between the `rx` (install + run) and `rxx`
(direct-run) CLI crates. It handles:

- **Source resolution** — distinguishes local files, local directories, and remote URLs; normalizes
  GitHub `blob` URLs to `raw.githubusercontent.com` before fetch.
- **Runtime detection** — reads a script's shebang line and maps it to a `Runtime` variant
  (rust-script, Python, JavaScript/TypeScript, Bash, Zsh, Fish, Nushell, Ruby).
- **Installation** — copies compatible scripts into an install directory, sets the executable bit,
  and records results; skips incompatible files when installing a directory.
- **Execution planning** — produces an `ExecutionPlan` (program + args) that callers can exec
  directly, without coupling the CLI crates to launcher details.
- **Command prefix** — wraps an existing `ExecutionPlan` with an outer command (e.g. `op run --`).

## Key types and traits

| Item | Kind | Purpose |
|---|---|---|
| `Runtime` | enum | Identifies the script's runtime (serializes as short codes: `rs`, `py`, `js`, …) |
| `RegistryEntry` | struct | Persisted record for one installed script |
| `InstalledScript` | struct | In-memory result of a single install operation |
| `InstallReport` | struct | Aggregates installed and skipped paths from a batch install |
| `ExecutionPlan` | struct | Normalized `(program, args)` pair ready for exec |
| `CommandPrefixConfig` | struct | Learned prefix mappings for the `rxx` prefix-learning feature |
| `RegistryStore` | trait | Persistence port: `list()` and `upsert()` — implemented by `rx-registry-json` |
| `RemoteScriptFetcher` | trait | Fetch port: `fetch(url) -> Result<String>` — implemented by the HTTP adapter |

## Core functions

```rust
// Install a script (file, directory, or URL) and record it in the registry.
pub fn install<R: RegistryStore, F: RemoteScriptFetcher>(
    request: &InstallRequest,
    registry: &mut R,
    fetcher: &F,
) -> Result<InstallReport>

// Build a launch plan for a previously-installed script looked up by name.
pub fn plan_installed_run<R: RegistryStore>(
    request: &RunRequest,
    registry: &R,
) -> Result<ExecutionPlan>

// Build a launch plan for a script path without installing it first (used by rxx).
pub fn plan_direct_run(request: &DirectRunRequest) -> Result<ExecutionPlan>

// Wrap an existing plan with an outer command prefix.
pub fn apply_command_prefix(plan: &ExecutionPlan, prefix: &[String]) -> Result<ExecutionPlan>
```

## Usage example

```rust
use rx_script_core::{install, plan_installed_run, InstallRequest, RunRequest};

// Implement the two ports for your storage and HTTP layer, then:
let report = install(
    &InstallRequest {
        source: "https://github.com/example/tools/blob/main/scripts/preflight.rs".into(),
        install_dir: install_dir.clone(),
    },
    &mut registry,
    &fetcher,
)?;

let plan = plan_installed_run(
    &RunRequest { name: "preflight".into(), args: vec![] },
    &registry,
)?;

// plan.program == "rust-script", plan.args == ["/path/to/preflight"]
std::process::Command::new(&plan.program).args(&plan.args).status()?;
```

## How it fits in the rx workspace

```
rx-script-core      ← domain logic (this crate)
rx-registry-json    ← implements RegistryStore + RemoteScriptFetcher, owns XDG path defaults
rx-install          ← `rx` CLI: install / list / run subcommands
rxx                 ← direct-run CLI, no install step
```

Keep runtime rules, naming conventions, and install semantics in this crate. CLI crates should
parse args, call planning functions, and exec the returned `ExecutionPlan` — nothing more.
