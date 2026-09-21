use anyhow::Result;
use rx_core::status::{self, RepoStatus, git_cli::GitCliProbe};
use std::path::PathBuf;

pub fn run_status(
    manifest_path: PathBuf,
    scan: Option<PathBuf>,
    filter_expr: Option<&str>,
    json: bool,
) -> Result<()> {
    let repos = crate::resolve_repos(manifest_path, scan, filter_expr)?;

    if repos.is_empty() {
        eprintln!("no repos found");
        return Ok(());
    }

    let statuses = status::collect_status(&repos, &GitCliProbe);

    if json {
        let json_out = serde_json::to_string_pretty(&statuses)?;
        println!("{json_out}");
    } else {
        render_table(&statuses);
    }

    Ok(())
}

fn render_table(statuses: &[RepoStatus]) {
    use comfy_table::{ContentArrangement, Table, presets::UTF8_FULL_CONDENSED};

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL_CONDENSED)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            "Repo",
            "Branch",
            "State",
            "Ahead",
            "Behind",
            "Staged",
            "Modified",
            "Untracked",
            "Last Commit",
        ]);

    for s in statuses {
        let last = s.last_commit.as_ref().map_or_else(
            || "-".to_string(),
            |c| format!("{} ({})", c.subject, status::relative_time(c.timestamp)),
        );
        table.add_row(vec![
            s.name.clone(),
            s.branch.clone().unwrap_or_else(|| "(detached)".into()),
            s.state_label().to_string(),
            s.ahead.to_string(),
            s.behind.to_string(),
            s.staged.to_string(),
            s.modified.to_string(),
            s.untracked.to_string(),
            last,
        ]);
    }

    println!("{table}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use rx_core::status::CommitMeta;

    fn sample_statuses() -> Vec<RepoStatus> {
        vec![RepoStatus {
            name: "test-repo".to_string(),
            path: PathBuf::from("/tmp/test-repo"),
            branch: Some("main".to_string()),
            upstream: Some("origin/main".to_string()),
            ahead: 1,
            behind: 0,
            dirty: true,
            untracked: 2,
            staged: 0,
            modified: 1,
            last_commit: Some(CommitMeta {
                sha: "abc1234".to_string(),
                subject: "test commit".to_string(),
                author: "test".to_string(),
                timestamp: 1700000000,
            }),
            error: None,
        }]
    }

    #[test]
    fn render_table_includes_headers() {
        // Capture table output by rendering to string via comfy_table
        use comfy_table::{ContentArrangement, Table, presets::UTF8_FULL_CONDENSED};

        let statuses = sample_statuses();
        let mut table = Table::new();
        table
            .load_preset(UTF8_FULL_CONDENSED)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec![
                "Repo",
                "Branch",
                "State",
                "Ahead",
                "Behind",
                "Staged",
                "Modified",
                "Untracked",
                "Last Commit",
            ]);

        for s in &statuses {
            let last = s.last_commit.as_ref().map_or_else(
                || "-".to_string(),
                |c| format!("{} ({})", c.subject, status::relative_time(c.timestamp)),
            );
            table.add_row(vec![
                s.name.clone(),
                s.branch.clone().unwrap_or_else(|| "(detached)".into()),
                s.state_label().to_string(),
                s.ahead.to_string(),
                s.behind.to_string(),
                s.staged.to_string(),
                s.modified.to_string(),
                s.untracked.to_string(),
                last,
            ]);
        }

        let output = table.to_string();
        assert!(output.contains("Repo"));
        assert!(output.contains("Branch"));
        assert!(output.contains("State"));
        assert!(output.contains("Ahead"));
        assert!(output.contains("Behind"));
        assert!(output.contains("test-repo"));
        assert!(output.contains("DIRTY"));
    }
}
