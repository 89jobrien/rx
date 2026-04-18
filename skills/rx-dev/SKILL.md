---
name: rx-dev
description: >
  Guides development work on the rx Rust workspace — adding runtimes, understanding crate
  boundaries and seams, and navigating the architecture. Triggers on "work on rx",
  "add a runtime to rx", "understand the rx architecture", "add a new shebang", "extend rx",
  "where does X go in rx", "how does rx work".
version: 0.1.0
---

# rx Dev Companion

`rx` is a small Rust workspace for script installation and direct execution. It installs
compatible scripts from local files, directories, or remote URLs into a local command directory,
records them in a JSON registry, and executes them by name. `rxx` uses the same runtime
selection logic to execute a script directly without installing.

## Workspace Layout

```
crates/
├── rx-core             — domain logic (source resolution, shebang detection, install rules,
│                         execution planning)
├── rx-registry-json    — JSON registry persistence, HTTP fetching adapters, XDG path defaults
├── rx-install          — `rx` CLI (install, list, run subcommands)
└── rxx                 — `rxx` CLI (direct-run without install)
```

**Keep runtime rules, naming rules, and install semantics in `rx-core`.** CLI crates parse args,
call planning/install functions, and execute the returned `ExecutionPlan`. They stay thin.

## Key Seams (Traits / Types)

| Seam | Crate | Purpose |
|------|-------|---------|
| `RegistryStore` | `rx-core` | Persistence port — list and upsert installed scripts |
| `RemoteScriptFetcher` | `rx-core` | Remote fetch port — used for URL installs |
| `ExecutionPlan` | `rx-core` | Normalized launch plan used by both `rx` and `rxx` |

Implementations of `RegistryStore` and `RemoteScriptFetcher` live in `rx-registry-json`. This
keeps I/O and HTTP out of `rx-core`.

## Build Commands

```bash
cargo build --workspace
cargo check -q --all-targets --workspace
cargo test --workspace
cargo clippy --all-targets --workspace -- -D warnings
cargo fmt --all -- --check
./examples/smoke.sh      # terse end-to-end verification
```

Always run from the workspace root.

## Adding a New Runtime

To support a new shebang family (e.g. `#!/usr/bin/env deno`):

1. **Find the shebang validation list** in `rx-core` — search for existing shebang strings to
   locate the acceptance logic.
2. **Add the new shebang pattern** to the validation function. Match the style of existing
   entries (exact prefix match or env-based match).
3. **Add a `Runtime` variant** (or equivalent discriminant) for the new runtime so
   `ExecutionPlan` can carry the right launch strategy.
4. **Add the runtime execution arm** — how to invoke the runtime binary with the script path
   and any forwarded args.
5. **Update `README.md`** — add the new shebang to the "Script Validation Rules" list.
6. **Write tests** — add at least one unit test in `rx-core` for the new shebang, and one
   integration/smoke test that installs and runs a real script with the new shebang.

Invariant: acceptance is shebang-based. If the file doesn't start with a recognised shebang,
`rx` rejects it. Keep that contract intact.

## Key Invariants

- **Shebang-based acceptance** — no extension sniffing, no content inspection beyond the first line.
- **Directory installs are partial-success** — skip incompatible files, fail only if nothing matched.
- **Command name from filename stem** — `foo.sh` and `foo.rs` collide by design.
- **GitHub blob URLs are normalised** — `github.com/.../blob/...` → `raw.githubusercontent.com/...`
  before fetch. Logic lives in `rx-registry-json`.
- **XDG-style defaults** — install dir `~/.config/rx/bin`, registry `~/.config/rx/registry.json`
  (or `$XDG_CONFIG_HOME/rx`).

## Where to Put New Code

| Type of change | Target crate |
|----------------|-------------|
| New runtime / shebang rule | `rx-core` |
| New registry backend (e.g. SQLite) | new crate implementing `RegistryStore` |
| New fetch adapter (e.g. S3) | `rx-registry-json` or new crate implementing `RemoteScriptFetcher` |
| New CLI subcommand | `rx-install` |
| Direct-run behaviour change | `rxx` |
| XDG path logic | `rx-registry-json` |

## Testing Strategy

- Prefer changing behaviour through tests in the crate that owns it.
- `rx-core` tests use in-memory fakes for `RegistryStore` and `RemoteScriptFetcher`.
- Integration tests go in `rx-install` or via `smoke.sh`/`demo.sh`.
- Never add logic to CLI crates to work around a missing test in `rx-core`.

## Additional Resources

- **`references/runtime-table.md`** — full list of supported shebangs and their `Runtime` variants
- **`references/architecture-notes.md`** — deeper notes on the port/adapter boundaries
