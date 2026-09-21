---
title: rx-install
type: crate
tags: [crate, cli, binary]
---

## rx-install

The `rx` CLI binary. Thin CLI layer over [[rx-core]] domain functions.

### Subcommands

- `install` -- install scripts from file/dir/URL
- `list` -- list installed scripts from registry
- `run` -- run an installed script by name
- `status` -- git status across repos (uses [[GitCliProbe]])
- `graph` -- Cargo dependency graph (uses [[FsCargoScanner]])
- `fan` -- fan-out command across repos

### External Commands

Unrecognized subcommands are treated as external commands. Shell aliases from
~/.zshrc and fish config are expanded. [[CommandPrefixConfig]] enables prefix
learning (e.g. `op plugin run --` or `dotenvx run --`).

### CLI-local Ports

- `PlanRunner` -- executes [[ExecutionPlan]] (adapter: ProcessRunner)
- `PrefixConfigStore` -- loads/saves prefix config (adapter: TomlPrefixConfigStore)
- `ShellAliasSource` -- discovers shell aliases (adapter: FsShellAliasSource)
