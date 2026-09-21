//! Git status collection across multiple repos.
//!
//! The [`GitProbe`] trait abstracts the git backend so callers can swap
//! between the CLI adapter ([`git_cli::GitCliProbe`]) and test fakes.

pub mod git_cli;

use crate::repo::RepoMeta;
use rayon::prelude::*;
use serde::Serialize;
use std::path::{Path, PathBuf};

// Domain types

#[derive(Debug, Clone, Serialize)]
pub struct RepoStatus {
    pub name: String,
    pub path: PathBuf,
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub dirty: bool,
    pub untracked: u32,
    pub staged: u32,
    pub modified: u32,
    pub last_commit: Option<CommitMeta>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommitMeta {
    pub sha: String,
    pub subject: String,
    pub author: String,
    pub timestamp: i64,
}

impl RepoStatus {
    /// Build an error-only status for repos that failed to probe.
    pub fn errored(name: &str, path: &Path, message: String) -> Self {
        Self {
            name: name.to_string(),
            path: path.to_path_buf(),
            branch: None,
            upstream: None,
            ahead: 0,
            behind: 0,
            dirty: false,
            untracked: 0,
            staged: 0,
            modified: 0,
            last_commit: None,
            error: Some(message),
        }
    }

    /// Human-readable state label for table rendering.
    pub fn state_label(&self) -> &'static str {
        if self.error.is_some() {
            "ERROR"
        } else if self.staged > 0 {
            "STAGED"
        } else if self.modified > 0 || self.untracked > 0 {
            "DIRTY"
        } else {
            "clean"
        }
    }
}

// Port

/// Probes a single git repo for branch, status, and last commit info.
pub trait GitProbe: Sync {
    /// Collects branch, worktree, and latest-commit metadata for a repository.
    fn probe(&self, repo_path: &Path) -> RepoStatus;
}

// Orchestrator

/// Collect git status for every repo in parallel.
pub fn collect_status(repos: &[RepoMeta], probe: &impl GitProbe) -> Vec<RepoStatus> {
    repos
        .par_iter()
        .map(|repo| {
            let mut status = probe.probe(&repo.path);
            status.name = repo.name.clone();
            status
        })
        .collect()
}

// Porcelain v2 line classification

/// A single classified line from `git status --porcelain=v2 --branch`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedLine {
    BranchHead(Option<String>),
    BranchUpstream(String),
    BranchAB { ahead: u32, behind: u32 },
    Changed { staged: bool, modified: bool },
    Untracked,
    Other,
}

/// Classify one line of porcelain v2 output into a [`ParsedLine`].
pub fn classify_line(line: &str) -> ParsedLine {
    if let Some(rest) = line.strip_prefix("# branch.head ") {
        if rest == "(detached)" {
            ParsedLine::BranchHead(None)
        } else {
            ParsedLine::BranchHead(Some(rest.to_string()))
        }
    } else if let Some(rest) = line.strip_prefix("# branch.upstream ") {
        ParsedLine::BranchUpstream(rest.to_string())
    } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
        let parts: Vec<&str> = rest.split_whitespace().collect();
        let ahead = parts
            .first()
            .and_then(|a| a.trim_start_matches('+').parse().ok())
            .unwrap_or(0);
        let behind = parts
            .get(1)
            .and_then(|b| b.trim_start_matches('-').parse().ok())
            .unwrap_or(0);
        ParsedLine::BranchAB { ahead, behind }
    } else if line.starts_with("1 ") || line.starts_with("2 ") {
        let xy = line.as_bytes().get(2..4).unwrap_or_default();
        let staged = xy.first().is_some_and(|&b| b != b'.');
        let modified = xy.get(1).is_some_and(|&b| b != b'.');
        ParsedLine::Changed { staged, modified }
    } else if line.starts_with("? ") {
        ParsedLine::Untracked
    } else {
        ParsedLine::Other
    }
}

/// Fold classified lines into a [`RepoStatus`].
pub fn build_status(lines: &[ParsedLine], repo_path: &Path) -> RepoStatus {
    let name = repo_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("?")
        .to_string();

    let mut branch: Option<String> = None;
    let mut upstream: Option<String> = None;
    let mut ahead: u32 = 0;
    let mut behind: u32 = 0;
    let mut staged: u32 = 0;
    let mut modified: u32 = 0;
    let mut untracked: u32 = 0;

    for line in lines {
        match line {
            ParsedLine::BranchHead(head) => branch = head.clone(),
            ParsedLine::BranchUpstream(up) => upstream = Some(up.clone()),
            ParsedLine::BranchAB {
                ahead: a,
                behind: b,
            } => {
                ahead = *a;
                behind = *b;
            }
            ParsedLine::Changed {
                staged: s,
                modified: m,
            } => {
                if *s {
                    staged += 1;
                }
                if *m {
                    modified += 1;
                }
            }
            ParsedLine::Untracked => untracked += 1,
            ParsedLine::Other => {}
        }
    }

    let dirty = staged > 0 || modified > 0 || untracked > 0;

    RepoStatus {
        name,
        path: repo_path.to_path_buf(),
        branch,
        upstream,
        ahead,
        behind,
        dirty,
        untracked,
        staged,
        modified,
        last_commit: None,
        error: None,
    }
}

// Relative time formatting

const SECS_PER_MINUTE: i64 = 60;
const SECS_PER_HOUR: i64 = 3_600;
const SECS_PER_DAY: i64 = 86_400;
const SECS_PER_WEEK: i64 = 604_800;

/// Format a unix timestamp as a relative time string (e.g. "3h", "2d").
pub fn relative_time(timestamp: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let delta = (now - timestamp).max(0);

    if delta < SECS_PER_MINUTE {
        format!("{delta}s")
    } else if delta < SECS_PER_HOUR {
        format!("{}m", delta / SECS_PER_MINUTE)
    } else if delta < SECS_PER_DAY {
        format!("{}h", delta / SECS_PER_HOUR)
    } else if delta < SECS_PER_WEEK {
        format!("{}d", delta / SECS_PER_DAY)
    } else {
        format!("{}w", delta / SECS_PER_WEEK)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct FakeProbe {
        statuses: Vec<RepoStatus>,
    }

    impl GitProbe for FakeProbe {
        fn probe(&self, repo_path: &Path) -> RepoStatus {
            self.statuses
                .iter()
                .find(|s| s.path == repo_path)
                .cloned()
                .unwrap_or_else(|| RepoStatus::errored("?", repo_path, "not found".into()))
        }
    }

    fn make_repo_meta(name: &str, path: &str) -> RepoMeta {
        RepoMeta {
            name: name.to_string(),
            path: PathBuf::from(path),
            role: None,
            language: None,
            default_branch: None,
            tags: Vec::new(),
        }
    }

    fn make_status(name: &str, path: &str, dirty: bool) -> RepoStatus {
        RepoStatus {
            name: name.to_string(),
            path: PathBuf::from(path),
            branch: Some("main".to_string()),
            upstream: Some("origin/main".to_string()),
            ahead: 0,
            behind: 0,
            dirty,
            untracked: 0,
            staged: 0,
            modified: if dirty { 1 } else { 0 },
            last_commit: Some(CommitMeta {
                sha: "abc1234".to_string(),
                subject: "test commit".to_string(),
                author: "test".to_string(),
                timestamp: 1700000000,
            }),
            error: None,
        }
    }

    #[test]
    fn collect_status_uses_probe_for_each_repo() {
        let repos = vec![
            make_repo_meta("alpha", "/tmp/alpha"),
            make_repo_meta("beta", "/tmp/beta"),
        ];
        let probe = FakeProbe {
            statuses: vec![
                make_status("alpha", "/tmp/alpha", false),
                make_status("beta", "/tmp/beta", true),
            ],
        };

        let results = collect_status(&repos, &probe);
        assert_eq!(results.len(), 2);
        let alpha = results.iter().find(|s| s.name == "alpha").unwrap();
        let beta = results.iter().find(|s| s.name == "beta").unwrap();
        assert!(!alpha.dirty);
        assert!(beta.dirty);
    }

    #[test]
    fn state_label_reflects_status() {
        let mut s = make_status("x", "/x", false);
        assert_eq!(s.state_label(), "clean");

        s.modified = 1;
        assert_eq!(s.state_label(), "DIRTY");

        s.modified = 0;
        s.staged = 1;
        assert_eq!(s.state_label(), "STAGED");

        s.error = Some("fail".into());
        assert_eq!(s.state_label(), "ERROR");
    }

    #[test]
    fn errored_status_has_error_set() {
        let s = RepoStatus::errored("bad", Path::new("/bad"), "broken".to_string());
        assert_eq!(s.error.as_deref(), Some("broken"));
        assert_eq!(s.state_label(), "ERROR");
    }

    // --- classify_line tests ---

    #[test]
    fn classify_line_branch_head() {
        assert_eq!(
            classify_line("# branch.head main"),
            ParsedLine::BranchHead(Some("main".to_string()))
        );
    }

    #[test]
    fn classify_line_detached() {
        assert_eq!(
            classify_line("# branch.head (detached)"),
            ParsedLine::BranchHead(None)
        );
    }

    #[test]
    fn classify_line_upstream() {
        assert_eq!(
            classify_line("# branch.upstream origin/main"),
            ParsedLine::BranchUpstream("origin/main".to_string())
        );
    }

    #[test]
    fn classify_line_ab() {
        assert_eq!(
            classify_line("# branch.ab +3 -1"),
            ParsedLine::BranchAB {
                ahead: 3,
                behind: 1
            }
        );
    }

    #[test]
    fn classify_line_changed_staged() {
        assert_eq!(
            classify_line("1 A. N... 000000 100644 100644 000000 abc123 new.rs"),
            ParsedLine::Changed {
                staged: true,
                modified: false
            }
        );
    }

    #[test]
    fn classify_line_changed_modified() {
        assert_eq!(
            classify_line("1 .M N... 100644 100644 100644 abc123 def456 src/main.rs"),
            ParsedLine::Changed {
                staged: false,
                modified: true
            }
        );
    }

    #[test]
    fn classify_line_untracked() {
        assert_eq!(classify_line("? foo.txt"), ParsedLine::Untracked);
    }

    #[test]
    fn classify_line_other() {
        assert_eq!(
            classify_line("# branch.oid abc1234567890"),
            ParsedLine::Other
        );
    }

    // --- build_status tests ---

    #[test]
    fn build_status_clean() {
        let lines = vec![
            ParsedLine::Other, // branch.oid
            ParsedLine::BranchHead(Some("main".to_string())),
            ParsedLine::BranchUpstream("origin/main".to_string()),
            ParsedLine::BranchAB {
                ahead: 0,
                behind: 0,
            },
        ];
        let s = build_status(&lines, Path::new("/tmp/test"));
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!(s.upstream.as_deref(), Some("origin/main"));
        assert_eq!(s.ahead, 0);
        assert_eq!(s.behind, 0);
        assert!(!s.dirty);
    }

    #[test]
    fn build_status_dirty() {
        let lines = vec![
            ParsedLine::BranchHead(Some("feat".to_string())),
            ParsedLine::BranchAB {
                ahead: 2,
                behind: 0,
            },
            ParsedLine::Changed {
                staged: true,
                modified: false,
            },
            ParsedLine::Changed {
                staged: false,
                modified: true,
            },
            ParsedLine::Untracked,
        ];
        let s = build_status(&lines, Path::new("/tmp/test"));
        assert!(s.dirty);
        assert_eq!(s.staged, 1);
        assert_eq!(s.modified, 1);
        assert_eq!(s.untracked, 1);
        assert_eq!(s.ahead, 2);
    }

    #[test]
    fn build_status_detached() {
        let lines = vec![ParsedLine::Other, ParsedLine::BranchHead(None)];
        let s = build_status(&lines, Path::new("/tmp/test"));
        assert!(s.branch.is_none());
    }

    #[test]
    fn relative_time_formats_correctly() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        assert_eq!(relative_time(now - 30), "30s");
        assert_eq!(relative_time(now - 300), "5m");
        assert_eq!(relative_time(now - 7200), "2h");
        assert_eq!(relative_time(now - 172800), "2d");
        assert_eq!(relative_time(now - 1209600), "2w");
    }
}
