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
```

Examples:

```bash
rx install scripts
rx install ./scripts/preflight.rs
rx install https://raw.githubusercontent.com/example/repo/main/script.rs
rx install <github-blob-url>
rx install ./scripts --install-dir ~/.local/bin
```

## Script Validation Rules

`rx` only installs files that start with one of these shebangs:

- `#!/usr/bin/env rust-script`
- `#!/usr/bin/rust-script`

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
- runtime, currently always `rust-script`
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

- `hello.rs` becomes `hello`
- `scripts/tools/preflight.rs` becomes `preflight`
- a remote URL ending in `script.rs` becomes `script`

## Current Scope

What `rx` does today:

- install scripts from files, directories, and URLs
- maintain a registry of installed commands
- replace registry entries on reinstall by command name

What is not implemented yet:

- listing installed commands from the CLI
- running installed commands through `rx`
- remote directory or repository installs
- metadata extraction beyond the placeholder description field
