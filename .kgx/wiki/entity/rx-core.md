---
title: rx-core
type: crate
tags: [crate, domain, ports]
---

## rx-core

Unified domain crate. Re-exports script types at crate root.

### Modules

- [[script module]] -- shebang detection, runtime enum, install/run planning
- [[repo module]] -- repos.toml manifest, scan discovery, filtering
- [[status module]] -- parallel git status via rayon + [[GitProbe]]
- [[graph module]] -- Cargo dependency graph via [[CargoScanner]]
- [[fan module]] -- fan-out command execution across repos

### Port Traits Defined Here

[[RegistryStore]], [[RemoteScriptFetcher]], [[ScriptReader]], [[ScriptWriter]],
[[DirectoryScanner]], [[RepoSource]], [[GitProbe]], [[CargoScanner]]
