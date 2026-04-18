---
name: rx-registry
description: >
  Lists and runs commands from the rx registry. Covers rx list, rx run, rxx direct-run, and
  registry inspection. Triggers on "list rx commands", "what's in the rx registry", "run a
  script with rx", "rx run preflight", "show installed rx scripts", "query the rx registry".
version: 0.1.0
---

# Querying and Running from the rx Registry

The rx registry (`~/.config/rx/registry.json`) tracks all installed commands. Use `rx list`
to inspect it and `rx run` to execute commands from it. Always run these via Bash on the
user's behalf.

## List Installed Commands

```bash
rx list
```

Output is tab-delimited: `name  description  installed-path  source`

If description is not set, the column shows `-`.

## Run an Installed Command

```bash
rx run <name>
# Pass args after --
rx run <name> -- --verbose
```

`rx run` resolves the command from the registry and executes it through the correct runtime
adapter for its stored `runtime` tag.

## Direct-Run Without Installing

```bash
rxx ./scripts/preflight.sh
rxx ./scripts/preflight.rs -- --arg value
```

`rxx` applies the same shebang validation and runtime selection as `rx run` but does not
touch the registry.

## Procedure

### To list installed commands

1. Run `rx list` via Bash.
2. Present the output as a table to the user.

### To run a command

1. Confirm the command name with the user or from `rx list` output.
2. Run `rx run <name>` (with any args) via Bash.
3. Stream/report stdout and stderr back to the user.

### To inspect the raw registry

Read the JSON file directly if structured inspection is needed:

```bash
cat ~/.config/rx/registry.json
```

Or with `$XDG_CONFIG_HOME` set:

```bash
cat "${XDG_CONFIG_HOME:-$HOME/.config}/rx/registry.json"
```

## Registry Fields

Each entry contains:

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Installed command name (filename stem) |
| `source` | string | Original install source (path or URL) |
| `install_path` | string | Absolute path of installed script |
| `runtime` | string | `rs`, `py`, `js`, `ts`, `sh`, `zsh`, `fish`, `nu`, `rb` |
| `description` | string \| null | Optional description (currently unset by install) |

## Common Issues

- **Command not found after install**: Ensure `~/.config/rx/bin` (or custom install dir) is
  on `$PATH`.
- **Wrong runtime executed**: Check the shebang in the installed file at `install_path`.
- **Stale registry entry**: Reinstall with `rx install <source>` to update the entry.
