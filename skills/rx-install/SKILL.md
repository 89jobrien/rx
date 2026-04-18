---
name: rx-install
description: >
  Installs scripts via rx on the user's behalf. Handles local files, directories, and remote
  URLs including GitHub blob links. Triggers on "install a script with rx", "rx install this
  file", "install from GitHub with rx", "install a directory of scripts", "rx install from URL".
version: 0.1.0
---

# Installing Scripts with rx

`rx install` copies a compatible script into the install directory (`~/.config/rx/bin` by
default) and records it in the JSON registry. Use this skill to install scripts on behalf
of the user — always run the command directly via Bash.

## Install Forms

```bash
# Single local file
rx install ./scripts/preflight.sh

# All compatible scripts in a directory (skips incompatible, fails if nothing matched)
rx install ./scripts

# Remote URL (raw or GitHub blob — blob URLs are auto-normalised)
rx install https://raw.githubusercontent.com/example/repo/main/script.rs
rx install https://github.com/example/repo/blob/main/script.rs

# Custom install directory
rx install ./scripts/preflight.sh --install-dir ~/.local/bin
```

## Procedure

1. **Confirm the source** — local path or URL provided by the user.
2. **Run `rx install <source>`** via Bash. Capture stdout and exit code.
3. **Report what was installed** — parse the output or run `rx list` to show the updated
   registry.
4. If the install fails with "no compatible scripts found", check the shebang of the target
   file (load the `rx-script-authoring` skill for authoring guidance).

## Example Session

User: "Install `./scripts/check.nu` with rx"

```bash
rx install ./scripts/check.nu
```

Then confirm:

```bash
rx list
```

Show the user the new registry entry.

## Reinstall Behaviour

Reinstalling a script with the same filename stem **updates** the existing registry entry —
it does not create a duplicate. Safe to run repeatedly.

## Custom Install Directory

When the user specifies a non-default install dir, pass `--install-dir`:

```bash
rx install ./scripts/check.nu --install-dir ~/.local/bin
```

The registry file is always at `~/.config/rx/registry.json` regardless of install dir
(unless `XDG_CONFIG_HOME` is set).

## Verifying After Install

After install, verify with:

```bash
rx list
```

Or run the installed command:

```bash
rx run <name>
```
