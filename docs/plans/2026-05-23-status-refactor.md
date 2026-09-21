# Status Module Refactor

**Date:** 2026-05-23
**Scope:** rx-core::status, rx-core::status::git_cli, rx-install (main.rs + new status.rs)
**Approach:** Parallel tracks -- fix rx-core complexity and extract main.rs code in one pass

## Goal

Reduce complexity in the status module (cognitive 54 in `parse_porcelain_v2`), fix IOSP
violations, extract magic numbers, and move presentation code out of main.rs into a
testable module. Target: all rustqual findings in the status vertical slice resolved.

## Architecture

### Classify-then-process parsing (rx-core::status)

Replace the monolithic `parse_porcelain_v2` with a two-phase approach:

1. **Classify** -- `classify_line(line: &str) -> ParsedLine` is a pure function that
   pattern-matches one line of `git status --porcelain=v2` output into a typed variant.
2. **Build** -- `build_status(lines: &[ParsedLine], repo_path: &Path) -> RepoStatus`
   folds classified lines into a `RepoStatus`. No string parsing here.

```rust
// rx-core::status (pub, so git_cli and tests can use it)

pub enum ParsedLine {
    BranchHead(Option<String>),          // None = detached
    BranchUpstream(String),
    BranchAB { ahead: u32, behind: u32 },
    Changed { staged: bool, modified: bool },
    Untracked,
    Other,
}

pub fn classify_line(line: &str) -> ParsedLine;
pub fn build_status(lines: &[ParsedLine], repo_path: &Path) -> RepoStatus;
```

`git_cli::parse_porcelain_v2` becomes a thin integration function:

```rust
fn parse_porcelain_v2(output: &str, repo_path: &Path) -> RepoStatus {
    let lines: Vec<ParsedLine> = output.lines().map(classify_line).collect();
    build_status(&lines, repo_path)
}
```

### Named time constants (rx-core::status)

```rust
const SECS_PER_MINUTE: i64 = 60;
const SECS_PER_HOUR: i64 = 3_600;
const SECS_PER_DAY: i64 = 86_400;
const SECS_PER_WEEK: i64 = 604_800;
```

Used in `relative_time`. Git format offsets (`2..4`, field count `4`) stay as-is --
they're spec-driven, not magic.

### Presentation extraction (rx-install)

Move `run_status` and `render_status_table` from `main.rs` into a new
`rx-install/src/status.rs`:

```rust
// rx-install/src/status.rs

pub fn run_status(
    manifest_path: PathBuf,
    scan: Option<PathBuf>,
    filter_expr: Option<&str>,
    json: bool,
) -> Result<()>;

fn render_table(statuses: &[RepoStatus]);
```

`main.rs` calls `status::run_status(...)` from the `Command::Status` arm.

### IOSP fix in git_cli::probe_impl

`probe_impl` currently mixes logic (error checking, string conversion) with calls
(Command::new, parse_porcelain_v2, probe_last_commit). After the refactor,
`parse_porcelain_v2` is already a pure integration call, so the IOSP violation
resolves naturally -- `probe_impl` becomes: run command, check error, parse output,
attach last commit. Each step is a call, no interleaved logic.

## Files Changed

| File                            | Action                                                            | Rustqual findings addressed                                                |
| ------------------------------- | ----------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `rx-core/src/status/mod.rs`     | Add `ParsedLine`, `classify_line`, `build_status`, time constants | 4x MAGIC_NUMBER, testability                                               |
| `rx-core/src/status/git_cli.rs` | Rewrite `parse_porcelain_v2` to use classify-then-process         | COGNITIVE 54, CYCLOMATIC 16, NESTING 7, LONG_FN 63, VIOLATION (probe_impl) |
| `rx-install/src/main.rs`        | Remove `run_status` + `render_status_table`, add `mod status`     | SRP_MODULE (reduces line count), TQ_UNTESTED x2                            |
| `rx-install/src/status.rs`      | **New** -- extracted presentation + orchestration                 | --                                                                         |

## Test Plan

### New unit tests (rx-core::status)

| Test                             | What it verifies                                           |
| -------------------------------- | ---------------------------------------------------------- |
| `classify_line_branch_head`      | `# branch.head main` -> `BranchHead(Some("main"))`         |
| `classify_line_detached`         | `# branch.head (detached)` -> `BranchHead(None)`           |
| `classify_line_upstream`         | `# branch.upstream origin/main` -> `BranchUpstream(...)`   |
| `classify_line_ab`               | `# branch.ab +3 -1` -> `BranchAB { ahead: 3, behind: 1 }`  |
| `classify_line_changed_staged`   | `1 A. N...` -> `Changed { staged: true, modified: false }` |
| `classify_line_changed_modified` | `1 .M N...` -> `Changed { staged: false, modified: true }` |
| `classify_line_untracked`        | `? foo.txt` -> `Untracked`                                 |
| `classify_line_other`            | `# branch.oid abc` -> `Other`                              |
| `build_status_clean`             | All-branch lines, no changes -> clean RepoStatus           |
| `build_status_dirty`             | Mix of Changed + Untracked -> correct counters, dirty=true |
| `build_status_detached`          | BranchHead(None) -> branch=None                            |

### Migrated tests (rx-core::status::git_cli)

Existing `parse_porcelain_v2_*` tests stay but now exercise the full
classify -> build pipeline. Same inputs, same assertions.

### New test (rx-install::status)

| Test                            | What it verifies                                           |
| ------------------------------- | ---------------------------------------------------------- |
| `render_table_includes_headers` | Output contains Repo, Branch, State, Ahead, Behind columns |

## Out of Scope

- `probe_last_commit` refactoring (low complexity, no findings beyond one magic number)
- `CommitMeta` changes
- `collect_status` orchestrator (already clean)
- Other modules (fan, graph, prefix learning) -- separate passes

## Implementation Order

1. Add time constants to `status/mod.rs`
2. Add `ParsedLine` enum + `classify_line` + `build_status` with tests
3. Rewrite `git_cli::parse_porcelain_v2` to use the new functions
4. Verify existing git_cli tests pass
5. Create `rx-install/src/status.rs`, move `run_status` + `render_status_table`
6. Update `main.rs` to delegate to `status::run_status`
7. Add `render_table` test
8. Run `rustqual --suggestions` and verify findings are resolved
