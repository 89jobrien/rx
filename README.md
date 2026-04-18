# rx

`rx` installs compatible scripts from local paths or remote URLs into a local command
directory and records what it installed in a JSON registry.

## What It Does

`rx` accepts three source types:

- a single local file
- a local directory, scanned recursively
- a remote HTTP or HTTPS URL for a single script

Installed scripts are copied into the target install directory, marked executable on Unix, and
written to a registry so the installed command set can be discovered later.

## Default Locations

Unless you override the install directory on the command line, `rx` uses XDG-style config paths:

- install directory: `~/.config/rx/bin`
- registry file: `~/.config/rx/registry.json`
- prefix config: `~/.config/rx/prefixes.toml`

If `XDG_CONFIG_HOME` is set, `rx` uses `$XDG_CONFIG_HOME/rx` instead of `~/.config/rx`.

## Installation

Install the CLIs from this repo:

```bash
cargo install --path crates/rx-install
cargo install --path crates/rxx
```

If the crates are published on crates.io and release artifacts are available, `cargo-binstall`
installs by package name:

```bash
cargo binstall rx-install
cargo binstall rxx
```

`cargo-binstall` resolves crate metadata from crates.io and then looks at the linked repository
releases for matching binary artifacts. It does not use the GitHub repo URL as the primary install
argument.

Or from crates.io when published:

```bash
cargo install rx-install
cargo install rxx
```

For local development, bootstrap the pinned toolchain with:

```bash
mise install
```

## Usage

```bash
rx install <source>
rx install <source> --install-dir <dir>
rx list
rx run <name> [-- <args...>]
rx <command> [args...]
rxx <script> [-- <args...>]
```

For local development from the workspace root:

```bash
cargo run --quiet -p rx-install --bin rx -- list
cargo run --quiet -p rx-install --bin rx -- run preflight
cargo run --quiet -p rxx --bin rxx -- ./scripts/preflight.sh
cargo run --quiet -p rxx --bin rxx -- ./.ctx/scripts/preflight.rs
```

Examples:

```bash
rx install scripts
rx install ./scripts/preflight.sh
rx install ./.ctx/scripts/preflight.rs
rx install https://raw.githubusercontent.com/example/repo/main/script.rs
rx install <github-blob-url>
rx install ./scripts --install-dir ~/.local/bin
rx list
rx run preflight
rxx ./scripts/preflight.sh
rxx ./.ctx/scripts/preflight.rs
```

For a full local walkthrough that leaves your real config untouched:

```bash
./examples/demo.sh
```

For a terse end-to-end verification script:

```bash
./examples/smoke.sh
```

The demo is a guided walkthrough. The smoke script is the same core flow with minimal narration.
They install [preflight.sh](/Users/joe/dev/rx/scripts/preflight.sh) as the default fast-path
command, keep [preflight.rs](/Users/joe/dev/rx/.ctx/scripts/preflight.rs) as a richer direct-run
comparison script, and then exercise `examples/scripts/` through the same temporary XDG config root.

`rx list` prints one tab-delimited row per installed command:

```text
name	description	installed-path	source
```

If a description is not yet known, `rx list` prints `-` in that column.

`rx run` resolves a command from the registry and executes it through the runtime adapter for its
stored runtime. `rxx` runs a compatible script directly without installing it first, using the same
runtime selection layer.

Unknown `rx` subcommands are treated as passthrough commands. That makes `rx gh issue list` a
valid invocation, and lets `rx` apply optional per-user launch prefixes before spawning the real
binary.

For passthrough commands, `rx` first auto-discovers simple command aliases from `~/.zshrc` and
`~/.config/fish/config.fish`, plus fish abbreviations declared with `abbr -a` or `abbr --add`.
That means aliases like `ocm='opencode -m ollama/gpt-mbx'` can be expanded before any prefix
handling or fallback learning runs.

`rx` only expands safe alias bodies that are plain command-and-argument lists. Aliases that depend
on shell control flow or builtins such as `cd`, `&&`, pipes, redirects, `source`, or `eval` are
ignored rather than partially emulated.

## Personal Prefix Learning

`rx` auto-discovers a few machine-local defaults before it looks at `~/.config/rx/prefixes.toml`:

- if `~/.config/op/plugins/*.json` exists, those configured command names default to
  `op plugin run --`
- if `dotenvx` is on PATH, common AI/tooling commands like `gemini`, `claude`, `codex`,
  `ollama`, `opencode`, `pi`, `deepagents`, and `toolz` default to `dotenvx run --`
- if `~/.config/mise/config.toml` declares AI npm tools, `rx` also infers matching command names
  from that global tool config when those binaries are installed

If `~/.config/rx/prefixes.toml` exists, `rx` merges it on top of those defaults before spawning
`rx run ...` plans or passthrough commands like `rx gh ...`.

`mappings` stores exact learned command wrappers. `candidate_prefixes` stores generic wrappers that
`rx` can try after an unmapped command fails. If a candidate wrapper succeeds and
`learn_on_successful_fallback = true`, `rx` persists that wrapper for the command so future runs go
straight to the learned mapping.

Example:

```toml
learn_on_successful_fallback = true
candidate_prefixes = [
  ["op", "plugin", "run", "--"],
  ["dotenvx", "run", "--"],
]

[mappings]
gh = ["op", "plugin", "run", "--"]
```

With that config, `rx gh issue list` executes as `op plugin run -- gh issue list`, and future
successful fallback discoveries are written back into the same file automatically.

Because fallback learning may retry a command after an initial failure, it is best suited to
idempotent commands or auth/bootstrap wrappers rather than destructive commands.

In practice the launch order is:

1. expand a safe shell alias or fish abbreviation if one matches
2. apply any discovered or user-configured exact mapping
3. run the command directly
4. if direct execution fails and fallback learning is enabled, try each candidate prefix until one
   succeeds, then persist that mapping

## Script Validation Rules

`rx` currently accepts files that start with one of these shebang families:

- `#!/usr/bin/env rust-script`
- `#!/usr/bin/rust-script`
- `#!/usr/bin/env python3` and related Python shebangs
- `#!/usr/bin/env node` / `#!/usr/bin/env bun` for JavaScript and TypeScript
- `#!/usr/bin/env bash`
- `#!/usr/bin/env zsh`
- `#!/usr/bin/env fish`
- `#!/usr/bin/env nu`
- `#!/usr/bin/env ruby`

Behavior by source type:

- local file: fails if the file is not a supported script
- local directory: installs matching files, skips non-matching files, and fails if nothing matched
- remote URL: downloads one file, validates it, and installs it

For GitHub URLs, `rx` automatically rewrites `github.com/.../blob/...` links into the equivalent
`raw.githubusercontent.com/...` download URL before fetching.

## Registry Format

Each install updates `registry.json` with one entry per command name. Reinstalling a script with the
same derived command name updates the existing entry instead of creating a duplicate.

Current registry entries include:

- command name
- original source
- installed path
- runtime, one of `rs`, `py`, `js`, `ts`, `sh`, `zsh`, `fish`, `nu`, or `rb`
- optional description, currently unset

Example:

```json
{
  "version": 1,
  "commands": [
    {
      "name": "preflight",
      "source": "./scripts/preflight.sh",
      "install_path": "/Users/alice/.config/rx/bin/preflight",
      "runtime": "sh",
      "description": null
    }
  ]
}
```

## Command Naming

The installed command name is derived from the script filename stem:

- `hello-rust.rs` becomes `hello-rust`
- `scripts/tools/preflight.sh` becomes `preflight`
- `scripts/tools/preflight.rs` also becomes `preflight`
- a remote URL ending in `script.rs` becomes `script`

Because command names come from the filename stem, `preflight.sh` and `preflight.rs` collide if you
install both into the same registry. The examples treat `preflight.sh` as the installed default and
run `preflight.rs` directly through `rxx`.

## Current Scope

What `rx` does today:

- install scripts from files, directories, and URLs
- maintain a registry of installed commands
- replace registry entries on reinstall by command name
- list installed commands from the CLI
- run installed compatible commands through `rx`
- execute compatible scripts directly through `rxx`

What is not implemented yet:

- remote directory or repository installs
- metadata extraction beyond the placeholder description field
- interpreter discovery and fallback logic beyond the current direct launcher mapping

## Workspace Layout

The repo is structured as a small Cargo workspace:

- `crates/rx-core`: runtime detection, install rules, and execution planning
- `crates/rx-registry-json`: JSON registry persistence and HTTP fetching adapters
- `crates/rx-install`: the `rx` CLI
- `crates/rxx`: the `rxx` CLI
