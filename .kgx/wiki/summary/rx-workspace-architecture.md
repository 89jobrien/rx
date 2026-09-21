---
title: rx workspace architecture
source_document: rx_architecture
tags: [summary, architecture, rust]
---

## rx Workspace Architecture

**Source:** codebase analysis of /Users/joe/dev/rx

### Workspace Structure

Four crates under `crates/`:

| Crate                | Role                                                                  |
| -------------------- | --------------------------------------------------------------------- |
| [[rx-core]]          | Domain logic: script, repo, status, graph, fan modules                |
| [[rx-registry-json]] | JSON persistence + HTTP fetch adapters                                |
| [[rx-install]]       | `rx` CLI binary (install/list/run/status/graph/fan + prefix learning) |
| [[rxx]]              | Direct-run CLI (execute script without installing)                    |

### Hexagonal Architecture

**Port traits** (in [[rx-core]]):

- [[RegistryStore]] -- persistence for installed scripts
- [[RemoteScriptFetcher]] -- URL fetching
- [[ScriptReader]], [[ScriptWriter]], [[DirectoryScanner]] -- filesystem
- [[RepoSource]] -- repo metadata listing
- [[GitProbe]] -- git status probing
- [[CargoScanner]] -- Cargo.toml scanning

**Adapters** (in [[rx-registry-json]]):

- [[JsonRegistryStore]] -> [[RegistryStore]]
- [[ReqwestFetcher]] -> [[RemoteScriptFetcher]]
- [[FsScriptReader]] -> [[ScriptReader]]
- [[FsScriptWriter]] -> [[ScriptWriter]]
- [[WalkdirScanner]] -> [[DirectoryScanner]]

**Adapters** (in [[rx-core]]):

- [[GitCliProbe]] -> [[GitProbe]]
- [[FsCargoScanner]] -> [[CargoScanner]]
- [[ManifestRepoSource]], [[ScanRepoSource]] -> [[RepoSource]]

### Key Types

- [[ExecutionPlan]] -- normalized {program, args} for script execution
- [[CommandPrefixConfig]] -- prefix learning (op plugin run, dotenvx run)
- [[Runtime]] -- enum of 9 supported shebang-detected runtimes
- [[RepoMeta]] -- repo entry with name/path/role/language/tags
- [[DepGraph]] -- cross-repo Cargo dependency graph
- [[RepoStatus]] -- git status per repo
- [[FanReport]] -- fan-out execution results

### Dependency Flow

```text
rx-install ----> rx-core
rx-install ----> rx-registry-json ----> rx-core
rxx -------> rx-core
rxx -------> rx-registry-json
```
