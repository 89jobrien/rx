# rx-install

The `rx` CLI — installs compatible scripts from local files, directories, or remote URLs, lists
installed scripts, and runs them by name.

## What it does

`rx` is the user-facing binary in the `rx` workspace. It delegates all domain logic to
`rx-script-core` and uses `rx-registry-json` for persistence. The CLI itself stays thin:
parse args, call core functions, execute the returned plan.

Beyond install/list/run, `rx` also acts as a transparent command dispatcher: unrecognized
subcommands are treated as external commands and forwarded directly, with optional command-prefix
wrapping (e.g. `op plugin run --` or `dotenvx run --`) applied automatically or learned on first
successful use.

## Subcommands

### `rx install <source>`

Installs compatible scripts from a local file, directory, or URL into the install directory.
Scripts are accepted based on shebang detection. Incompatible files are skipped with a warning;
the command fails if nothing compatible was found.

```
rx install ./scripts/
rx install ./deploy.sh
rx install https://raw.githubusercontent.com/org/repo/main/tool.sh
```

Options:
- `--install-dir <DIR>` — override destination (default: `$XDG_CONFIG_HOME/rx/bin`)

### `rx list`

Prints all installed scripts from the registry.

```
rx list
```

Options:
- `--registry-path <FILE>` — override registry location (default: `$XDG_CONFIG_HOME/rx/registry.json`)

### `rx run <name> [args...]`

Runs a previously installed script by its registry name.

```
rx run deploy --env staging
```

Options:
- `--registry-path <FILE>` — override registry location

### `rx <external-command> [args...]`

Any unrecognized subcommand is dispatched as an external command. Shell aliases from `~/.zshrc`
and `~/.config/fish/config.fish` are resolved before dispatch. A command-prefix config
(`prefixes.toml`) is consulted to wrap commands with known prefixes (e.g. 1Password plugin
runner, dotenvx). When `learn_on_successful_fallback = true`, a working prefix is saved
automatically for future calls.

```
rx gh issue list
rx gemini "explain this"
```

## Configuration

Default paths follow XDG conventions, rooted at `$XDG_CONFIG_HOME/rx` (or `~/.config/rx`):

| File              | Purpose                                      |
|-------------------|----------------------------------------------|
| `bin/`            | Installed script destination                 |
| `registry.json`   | Script registry (managed by rx-registry-json)|
| `prefixes.toml`   | Command-prefix mappings and candidate list   |

## Workspace role

`rx-install` is one of four crates in the `rx` workspace:

- `rx-script-core` — domain logic (shebang detection, install rules, execution planning)
- `rx-registry-json` — JSON registry persistence and HTTP fetch adapters
- **`rx-install`** — the `rx` binary (this crate)
- `rxx` — direct-run binary; executes a compatible script without installing it first
