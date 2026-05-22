//! Git status collection across multiple repos.
//!
//! The [`GitProbe`] trait abstracts the git backend so callers can swap
//! between the CLI adapter ([`git_cli::GitCliProbe`]) and test fakes.

pub mod git_cli;

use crate::repo::RepoMeta;
use rayon::prelude::*;
use serde::Serialize;
use std::path::{Path, PathBuf};

// =========================================================================
// Domain types
// =========================================================================

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

// =========================================================================
// Port
// =========================================================================

/// Probes a single git repo for branch, status, and last commit info.
pub trait GitProbe: Sync {
    fn probe(&self, repo_path: &Path) -> RepoStatus;
}

// =========================================================================
// Orchestrator
// =========================================================================

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

// =========================================================================
// Relative time formatting
// =========================================================================

/// Format a unix timestamp as a relative time string (e.g. "3h", "2d").
pub fn relative_time(timestamp: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let delta = (now - timestamp).max(0);

    if delta < 60 {
        format!("{delta}s")
    } else if delta < 3600 {
        format!("{}m", delta / 60)
    } else if delta < 86400 {
        format!("{}h", delta / 3600)
    } else if delta < 604800 {
        format!("{}d", delta / 86400)
    } else {
        format!("{}w", delta / 604800)
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
