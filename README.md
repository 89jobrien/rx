# rx

`rx` installs `rust-script` programs from local paths or remote URLs into a local command
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

If `XDG_CONFIG_HOME` is set, `rx` uses `$XDG_CONFIG_HOME/rx` instead of `~/.config/rx`.

## Installation

Install the CLI from this repo:

```bash
cargo install --path .
```

Or from crates.io when published:

```bash
cargo install rx-install
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
rxx <script> [-- <args...>]
```

Examples:

```bash
rx install scripts
rx install ./scripts/preflight.rs
rx install https://raw.githubusercontent.com/example/repo/main/script.rs
rx install <github-blob-url>
rx install ./scripts --install-dir ~/.local/bin
rx list
rx run preflight -- --check
rxx ./scripts/preflight.rs -- --check
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
Both install `examples/scripts/` into a temporary XDG config root, show the generated registry
entries, run installed Rust and Python commands through `rx run`, and then execute JavaScript,
TypeScript, Bash, Zsh, Fish, and Nushell examples directly through `rxx`.

`rx list` prints one tab-delimited row per installed command:

```text
name	description	installed-path	source
```

If a description is not yet known, `rx list` prints `-` in that column.

`rx run` resolves a command from the registry and executes it through the runtime adapter for its
stored runtime. `rxx` runs a compatible script directly without installing it first, using the same
runtime selection layer.

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

Behavior by source type:

- local file: fails if the file is not a valid `rust-script`
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
- runtime, one of `rust-script`, `python`, `javascript`, `typescript`, `bash`, `zsh`, `fish`, or `nushell`
- optional description, currently unset

Example:

```json
{
  "version": 1,
  "commands": [
    {
      "name": "preflight",
      "source": "./scripts/preflight.rs",
      "install_path": "/Users/alice/.config/rx/bin/preflight",
      "runtime": "rust-script",
      "description": null
    }
  ]
}
```

## Command Naming

The installed command name is derived from the script filename stem:

- `hello-rust.rs` becomes `hello-rust`
- `scripts/tools/preflight.rs` becomes `preflight`
- a remote URL ending in `script.rs` becomes `script`

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
