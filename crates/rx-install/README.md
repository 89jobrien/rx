# rx-install

`rx-install` is the package that builds the `rx` binary. The CLI installs compatible scripts,
lists and runs registered commands, reports on multiple git repositories, builds cross-repository
Cargo dependency graphs, fans commands out across repositories, and dispatches otherwise unknown
subcommands as external programs.

The binary wires domain operations from `rx-core` to the JSON, HTTP, and filesystem adapters in
`rx-registry-json`.

## Contents

- [Installation and Local Use](#installation-and-local-use)
- [Default Files](#default-files)
- [Script Commands](#script-commands)
- [Multi-Repository Commands](#multi-repository-commands)
- [External Command Dispatch](#external-command-dispatch)
- [Shell Completion](#shell-completion)
- [Workspace Role](#workspace-role)
- [Development and Testing](#development-and-testing)

## Installation and Local Use

From the repository:

```bash
cargo install --path crates/rx-install
cargo run --quiet -p rx-install --bin rx -- list
```

The package has no feature flags. Runtime commands used by installed scripts must be available on
`PATH`; depending on the shebang, plans use `rust-script`, `uv run`, `bun`, `bash`, `zsh`, `fish`,
`nu`, or `ruby`.

## Default Files

Defaults are rooted at `$XDG_CONFIG_HOME/rx`, or `$HOME/.config/rx` when `XDG_CONFIG_HOME` is not
set:

| Path            | Purpose                                                  |
| --------------- | -------------------------------------------------------- |
| `bin/`          | Default installed-script destination                     |
| `registry.json` | Installed command registry                               |
| `prefixes.toml` | External-command prefix mappings and fallback candidates |
| `repos.toml`    | Optional multi-repository manifest                       |

`--prefix-config <FILE>` is a global option and defaults to `prefixes.toml` above.

## Script Commands

### `rx install <SOURCE>`

Install one local file, all compatible files recursively found under a local directory, or one
HTTP(S) URL:

```bash
rx install ./scripts/preflight.sh
rx install ./examples/scripts --install-dir "$HOME/.local/bin"
rx install https://raw.githubusercontent.com/owner/repo/main/tool.py
rx install https://github.com/owner/repo/blob/main/tool.py
```

`--install-dir <DIR>` overrides the script destination. The registry still uses the default
`registry.json`. GitHub blob URLs are normalized to raw-content URLs. Command names come from file
stems, so `tool.py` and `tool.sh` both register as `tool`; the later upsert replaces the earlier
entry.

A single file or URL fails when its first line is not a supported shebang. Directory installation
skips incompatible files, reports them on stderr, and fails if no compatible file is found. On
Unix, installed files are written with mode `0755`.

### `rx list`

```bash
rx list
rx list --registry-path /tmp/rx/registry.json
```

Each entry is a tab-delimited row with name, description (or `-`), installed path, and source. A
missing registry is treated as empty. `--registry-path <FILE>` selects another registry.

### `rx run <NAME> [ARGS]...`

Resolve a command by name from the registry and run it through the launcher selected at install
time:

```bash
rx run preflight
rx run hello-python -- --name rx
rx run deploy --registry-path /tmp/rx/registry.json -- --env staging
```

`--registry-path <FILE>` selects another registry. Remaining arguments are forwarded to the
script; `--` is useful when the script arguments resemble `rx` options. The child inherits stdin,
stdout, and stderr, and `rx` exits with the child's status code.

## Multi-Repository Commands

`status`, `graph`, and `fan` share these selectors:

| Option                | Behavior                                                       |
| --------------------- | -------------------------------------------------------------- |
| `--manifest <FILE>`   | Load repository metadata; defaults to `repos.toml` above       |
| `--scan <DIR>`        | Discover nested git repositories instead of loading a manifest |
| `-f, --filter <EXPR>` | Keep repositories matching one expression                      |

Filters support `tag=value`, `role=value`, `language=value`, `name=value`, and `name~glob`. The
glob syntax supports `*`. Scan mode ignores directories named `target`, `node_modules`, and `.git`.

Example manifest:

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

### `rx status`

```bash
rx status --scan "$HOME/dev"
rx status --manifest "$HOME/.config/rx/repos.toml" --filter tag=rust
rx status --scan "$HOME/dev" --json
```

The default table reports repository, branch, state, ahead/behind counts, staged, modified, and
untracked counts, and the latest commit subject with relative age. `--json` emits the complete
`RepoStatus` array. Git data comes from the local `git` CLI; the command does not fetch remotes.

### `rx graph`

```bash
rx graph --scan "$HOME/dev"
rx graph --scan "$HOME/dev" --who-uses rx-core
rx graph --scan "$HOME/dev" --deps rx-install
rx graph --scan "$HOME/dev" --format mermaid
```

The graph contains Cargo packages found in selected repositories and edges to other packages in
that same scanned set. `--who-uses <PKG>` and `--deps <PKG>` return transitive queries. Recognized
rendering values are `tree`, `json`, and `mermaid`. The default is `tree`, and unknown values also
fall back to tree output rather than being rejected.

### `rx fan -- <COMMAND>...`

```bash
rx fan --scan "$HOME/dev" -- git status --short
rx fan --scan "$HOME/dev" --filter tag=rust -c 4 --timeout 30 -- cargo test
rx fan --scan "$HOME/dev" --dry-run -- git fetch --all --prune
rx fan --scan "$HOME/dev" --output json -- git log --oneline -1
```

The separator before the command is required. Each process runs with its repository as the working
directory. `-c, --concurrency <N>` defaults to the host CPU count, `--timeout <SECS>` sets a
per-repository timeout, `--fail-fast` cancels work not yet started after a failure, and `--dry-run`
prints targets without spawning commands. Recognized `--output` values are `grouped` and `json`.
The default is `grouped`, and unknown values also fall back to grouped output rather than being
rejected. Any failure makes `rx fan` exit with status 1.

## External Command Dispatch

An unknown subcommand is treated as a program and argument vector:

```bash
rx gh issue list
rx opencode -m ollama/gpt-mbx
```

Before execution, `rx` loads simple command aliases from `~/.zshrc` and aliases or `abbr -a` /
`abbr --add` entries from `~/.config/fish/config.fish`. Expansions containing shell control flow,
redirection, command substitution, pipes, or selected shell builtins are ignored. Commands are
spawned directly rather than evaluated by a shell.

Next, `rx` applies an exact mapping from `prefixes.toml`. Without a mapping it first executes the
base command. If that fails and fallback learning is enabled, each candidate prefix is tried until
one succeeds; that mapping is then written back to the file. Because a failed command can be run
again, fallback learning is appropriate only when retrying is safe.

```toml
learn_on_successful_fallback = true
candidate_prefixes = [
  ["op", "plugin", "run", "--"],
  ["dotenvx", "run", "--"],
]

[mappings]
gh = ["op", "plugin", "run", "--"]
```

Discovered defaults are merged underneath the file's mappings. Configured 1Password plugin JSON
files provide `op plugin run --` mappings. If `dotenvx` is installed, selected installed AI tools
and recognized AI npm tools in the global mise config receive `dotenvx run --` mappings.

## Shell Completion

`rx completions` prints a Nushell completion definition to stdout:

```bash
rx completions > rx-completions.nu
```

This path is handled before normal Clap subcommand parsing and currently emits Nushell only.

## Workspace Role

```text
rx-core              domain types, planning, and multi-repo operations
rx-registry-json     persistence, HTTP, and filesystem adapters
rx-install           the rx CLI (this package)
rx-rxx               the rxx direct-run CLI
rx-runner            standalone process runner; not currently used by rx
```

## Development and Testing

From the workspace root:

```bash
cargo check -p rx-install
cargo fmt --all -- --check
cargo clippy -p rx-install --all-targets -- -D warnings
cargo test -p rx-install
cargo run --quiet -p rx-install --bin rx -- --help
```

Unit tests cover prefix discovery and learning, safe alias parsing, config persistence, and status
table construction. Integration tests run install/list and install/run flows in isolated temporary
XDG roots. The repository-level `examples/demo.sh` and `examples/smoke.sh` exercise the broader
script workflow when all documented runtime tools are installed.
