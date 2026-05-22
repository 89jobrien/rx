//! Git CLI adapter for [`GitProbe`].
//!
//! Shells out to `git status --porcelain=v2 --branch` and `git log -1`
//! to collect per-repo status. This is the default adapter; callers can
//! swap in a fake for testing.

use super::{CommitMeta, GitProbe, RepoStatus};
use std::path::Path;
use std::process::Command;

/// Adapter that shells out to `git` for branch, status, and commit info.
pub struct GitCliProbe;

impl GitProbe for GitCliProbe {
    fn probe(&self, repo_path: &Path) -> RepoStatus {
        match probe_impl(repo_path) {
            Ok(status) => status,
            Err(e) => RepoStatus::errored(
                repo_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("?"),
                repo_path,
                e.to_string(),
            ),
        }
    }
}

fn probe_impl(repo_path: &Path) -> Result<RepoStatus, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["status", "--porcelain=v2", "--branch"])
        .current_dir(repo_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git status failed: {}", stderr.trim()).into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut status = parse_porcelain_v2(&stdout, repo_path);
    status.last_commit = probe_last_commit(repo_path).ok();
    Ok(status)
}

fn parse_porcelain_v2(output: &str, repo_path: &Path) -> RepoStatus {
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

    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            if rest != "(detached)" {
                branch = Some(rest.to_string());
            }
        } else if let Some(rest) = line.strip_prefix("# branch.upstream ") {
            upstream = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if let Some(a) = parts.first() {
                ahead = a.trim_start_matches('+').parse().unwrap_or(0);
            }
            if let Some(b) = parts.get(1) {
                behind = b.trim_start_matches('-').parse().unwrap_or(0);
            }
        } else if line.starts_with("1 ") || line.starts_with("2 ") {
            // Changed entry: XY format at position 2..4
            let xy: Vec<u8> = line.as_bytes().get(2..4).unwrap_or_default().to_vec();
            if xy.len() == 2 {
                if xy[0] != b'.' {
                    staged += 1;
                }
                if xy[1] != b'.' {
                    modified += 1;
                }
            }
        } else if line.starts_with("? ") {
            untracked += 1;
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

fn probe_last_commit(repo_path: &Path) -> Result<CommitMeta, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["log", "-1", "--format=%H%n%s%n%an%n%at"])
        .current_dir(repo_path)
        .output()?;

    if !output.status.success() {
        return Err("git log failed".into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    if lines.len() < 4 {
        return Err("unexpected git log output".into());
    }

    Ok(CommitMeta {
        sha: lines[0][..7.min(lines[0].len())].to_string(),
        subject: lines[1].to_string(),
        author: lines[2].to_string(),
        timestamp: lines[3].parse().unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_porcelain_v2_clean_repo() {
        let output = "\
# branch.oid abc1234567890
# branch.head main
# branch.upstream origin/main
# branch.ab +0 -0
";
        let status = parse_porcelain_v2(output, &PathBuf::from("/tmp/test"));
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!(status.upstream.as_deref(), Some("origin/main"));
        assert_eq!(status.ahead, 0);
        assert_eq!(status.behind, 0);
        assert!(!status.dirty);
        assert_eq!(status.staged, 0);
        assert_eq!(status.modified, 0);
        assert_eq!(status.untracked, 0);
    }

    #[test]
    fn parse_porcelain_v2_dirty_repo() {
        let output = "\
# branch.oid abc1234567890
# branch.head feature
# branch.upstream origin/feature
# branch.ab +3 -1
1 .M N... 100644 100644 100644 abc123 def456 src/main.rs
1 A. N... 000000 100644 100644 000000 abc123 new_file.rs
? untracked.txt
";
        let status = parse_porcelain_v2(output, &PathBuf::from("/tmp/test"));
        assert_eq!(status.branch.as_deref(), Some("feature"));
        assert_eq!(status.ahead, 3);
        assert_eq!(status.behind, 1);
        assert!(status.dirty);
        assert_eq!(status.staged, 1); // A.
        assert_eq!(status.modified, 1); // .M
        assert_eq!(status.untracked, 1);
    }

    #[test]
    fn parse_porcelain_v2_detached_head() {
        let output = "\
# branch.oid abc1234567890
# branch.head (detached)
";
        let status = parse_porcelain_v2(output, &PathBuf::from("/tmp/test"));
        assert!(status.branch.is_none());
        assert!(status.upstream.is_none());
    }
}
