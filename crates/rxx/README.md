# rxx

Execute compatible scripts directly, without installing them.

## What it does

`rxx` runs a single script by inspecting its shebang line, selecting the appropriate runtime, and
exec-ing it immediately. No registry entry is written, no install directory is involved. It is the
ephemeral counterpart to `rx`: same runtime-selection logic, zero side effects.

Use `rxx` when you want to run a script once, test it before committing it to the registry, or
invoke a script from a path where install-then-run would be wasteful.

## Usage

```
rxx <script> [args...]
```

All arguments after the script path are forwarded verbatim to the script.

### Examples

```bash
# Run a shell script directly
rxx ./scripts/deploy.sh --env staging

# Run a Rust script (e.g. via rust-script shebang)
rxx ./tools/codegen.rs --dry-run

# Pass flags that look like options (hyphen values are handled correctly)
rxx ./check.sh --verbose --fail-fast
```

Exit code is propagated from the child process.

## How it fits in the workspace

The `rx` workspace contains four crates:

| Crate              | Role                                                              |
| ------------------ | ----------------------------------------------------------------- |
| `rx-script-core`   | Domain logic: shebang detection, runtime selection, plan building |
| `rx-registry-json` | JSON registry persistence and HTTP fetch adapters                 |
| `rx-install`       | `rx` CLI — install, list, and run from the registry              |
| `rx-rxx`           | `rxx` CLI — direct-run without touching the registry             |

`rxx` calls `plan_direct_run` from `rx-script-core` to build an `ExecutionPlan`, then spawns
the resolved program with inherited stdio. The runtime rules are identical to those used by `rx`;
only the registry step is skipped.

## Supported runtimes

Runtime support is determined by the shebang line. See `rx-script-core` for the full list of
accepted interpreters and how they are resolved.
