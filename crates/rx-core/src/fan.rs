//! Fan-out command execution across multiple repos.
//!
//! Runs a command in every repo (or filtered subset) with configurable
//! parallelism, per-repo timeout, and grouped or JSON output.

use crate::repo::RepoMeta;
use serde::Serialize;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

// =========================================================================
// Domain types
// =========================================================================

#[derive(Debug, Clone, Serialize)]
pub struct FanResult {
    pub repo: String,
    pub path: PathBuf,
    pub argv: Vec<String>,
    pub exit_code: Option<i32>,
    pub duration_ms: u128,
    pub stdout: String,
    pub stderr: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl FanResult {
    pub fn passed(&self) -> bool {
        self.exit_code == Some(0)
    }

    pub fn timed_out(&self) -> bool {
        self.error
            .as_deref()
            .is_some_and(|e| e.contains("timed out"))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FanReport {
    pub command: Vec<String>,
    pub results: Vec<FanResult>,
    pub passed: usize,
    pub failed: usize,
    pub timed_out: usize,
    pub total_ms: u128,
}

impl FanReport {
    pub fn from_results(command: Vec<String>, results: Vec<FanResult>, total_ms: u128) -> Self {
        let passed = results.iter().filter(|r| r.passed()).count();
        let timed_out = results.iter().filter(|r| r.timed_out()).count();
        let failed = results.len() - passed;
        Self {
            command,
            results,
            passed,
            failed,
            timed_out,
            total_ms,
        }
    }
}

// =========================================================================
// Configuration
// =========================================================================

#[derive(Debug, Clone)]
pub struct FanConfig {
    pub command: Vec<String>,
    pub concurrency: usize,
    pub timeout: Option<Duration>,
    pub fail_fast: bool,
}

// =========================================================================
// Execution
// =========================================================================

/// Run `config.command` in each repo's directory, with bounded parallelism.
pub fn fan_out(repos: &[RepoMeta], config: &FanConfig) -> FanReport {
    let start = Instant::now();
    let cancelled = AtomicBool::new(false);

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(config.concurrency)
        .build()
        .expect("failed to build thread pool");

    let results: Vec<FanResult> = pool.install(|| {
        use rayon::prelude::*;
        repos
            .par_iter()
            .map(|repo| {
                if config.fail_fast && cancelled.load(Ordering::Relaxed) {
                    return FanResult {
                        repo: repo.name.clone(),
                        path: repo.path.clone(),
                        argv: config.command.clone(),
                        exit_code: None,
                        duration_ms: 0,
                        stdout: String::new(),
                        stderr: String::new(),
                        error: Some("cancelled (fail-fast)".into()),
                    };
                }

                let result = run_one(repo, &config.command, config.timeout);

                if config.fail_fast && !result.passed() {
                    cancelled.store(true, Ordering::Relaxed);
                }

                result
            })
            .collect()
    });

    let total_ms = start.elapsed().as_millis();
    FanReport::from_results(config.command.clone(), results, total_ms)
}

fn run_one(repo: &RepoMeta, argv: &[String], timeout: Option<Duration>) -> FanResult {
    let (program, args) = match argv.split_first() {
        Some((p, a)) => (p, a),
        None => {
            return FanResult {
                repo: repo.name.clone(),
                path: repo.path.clone(),
                argv: argv.to_vec(),
                exit_code: None,
                duration_ms: 0,
                stdout: String::new(),
                stderr: String::new(),
                error: Some("empty command".into()),
            };
        }
    };

    let start = Instant::now();
    let child = Command::new(program)
        .args(args)
        .current_dir(&repo.path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            return FanResult {
                repo: repo.name.clone(),
                path: repo.path.clone(),
                argv: argv.to_vec(),
                exit_code: None,
                duration_ms: start.elapsed().as_millis(),
                stdout: String::new(),
                stderr: String::new(),
                error: Some(format!("spawn error: {e}")),
            };
        }
    };

    if let Some(dur) = timeout {
        match wait_with_timeout(&mut child, dur) {
            WaitResult::Done(output) => FanResult {
                repo: repo.name.clone(),
                path: repo.path.clone(),
                argv: argv.to_vec(),
                exit_code: output.status.code(),
                duration_ms: start.elapsed().as_millis(),
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                error: None,
            },
            WaitResult::TimedOut => {
                let _ = child.kill();
                let _ = child.wait();
                FanResult {
                    repo: repo.name.clone(),
                    path: repo.path.clone(),
                    argv: argv.to_vec(),
                    exit_code: None,
                    duration_ms: start.elapsed().as_millis(),
                    stdout: String::new(),
                    stderr: String::new(),
                    error: Some(format!("timed out after {}s", dur.as_secs())),
                }
            }
        }
    } else {
        match child.wait_with_output() {
            Ok(output) => FanResult {
                repo: repo.name.clone(),
                path: repo.path.clone(),
                argv: argv.to_vec(),
                exit_code: output.status.code(),
                duration_ms: start.elapsed().as_millis(),
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                error: None,
            },
            Err(e) => FanResult {
                repo: repo.name.clone(),
                path: repo.path.clone(),
                argv: argv.to_vec(),
                exit_code: None,
                duration_ms: start.elapsed().as_millis(),
                stdout: String::new(),
                stderr: String::new(),
                error: Some(format!("wait error: {e}")),
            },
        }
    }
}

enum WaitResult {
    Done(std::process::Output),
    TimedOut,
}

fn wait_with_timeout(child: &mut std::process::Child, timeout: Duration) -> WaitResult {
    // Poll-based timeout: check every 50ms
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => {
                // Child finished -- collect output
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut out) = child.stdout.take() {
                    use std::io::Read;
                    let _ = out.read_to_end(&mut stdout);
                }
                if let Some(mut err) = child.stderr.take() {
                    use std::io::Read;
                    let _ = err.read_to_end(&mut stderr);
                }
                return WaitResult::Done(std::process::Output {
                    status: _status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    return WaitResult::TimedOut;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return WaitResult::TimedOut,
        }
    }
}

// =========================================================================
// Rendering
// =========================================================================

/// Render grouped output (default).
pub fn render_grouped(report: &FanReport) -> String {
    let mut out = String::new();
    let cmd_str = report.command.join(" ");

    for r in &report.results {
        let icon = if r.passed() {
            "ok"
        } else if r.timed_out() {
            "TIMEOUT"
        } else {
            "FAIL"
        };
        let duration = format!("{:.1}s", r.duration_ms as f64 / 1000.0);
        out.push_str(&format!(
            "== {}  {}  {}  {}\n",
            r.repo, icon, duration, cmd_str
        ));

        if !r.stdout.is_empty() && !r.passed() {
            for line in r.stdout.lines().take(20) {
                out.push_str(&format!("   {line}\n"));
            }
        }
        if !r.stderr.is_empty() && !r.passed() {
            for line in r.stderr.lines().take(20) {
                out.push_str(&format!("   {line}\n"));
            }
        }
        if let Some(err) = &r.error {
            out.push_str(&format!("   error: {err}\n"));
        }
    }

    out.push_str(&format!(
        "\n{} repos | {} passed | {} failed | {} timed out | {:.1}s total\n",
        report.results.len(),
        report.passed,
        report.failed,
        report.timed_out,
        report.total_ms as f64 / 1000.0,
    ));

    out
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn meta(name: &str, path: &str) -> RepoMeta {
        RepoMeta {
            name: name.to_string(),
            path: PathBuf::from(path),
            role: None,
            language: None,
            default_branch: None,
            tags: Vec::new(),
        }
    }

    #[test]
    fn fan_result_passed_and_timed_out() {
        let r = FanResult {
            repo: "a".into(),
            path: PathBuf::from("/a"),
            argv: vec!["true".into()],
            exit_code: Some(0),
            duration_ms: 10,
            stdout: String::new(),
            stderr: String::new(),
            error: None,
        };
        assert!(r.passed());
        assert!(!r.timed_out());

        let r2 = FanResult {
            error: Some("timed out after 5s".into()),
            exit_code: None,
            ..r.clone()
        };
        assert!(!r2.passed());
        assert!(r2.timed_out());
    }

    #[test]
    fn fan_report_counts() {
        let results = vec![
            FanResult {
                repo: "a".into(),
                path: PathBuf::from("/a"),
                argv: vec!["true".into()],
                exit_code: Some(0),
                duration_ms: 10,
                stdout: String::new(),
                stderr: String::new(),
                error: None,
            },
            FanResult {
                repo: "b".into(),
                path: PathBuf::from("/b"),
                argv: vec!["false".into()],
                exit_code: Some(1),
                duration_ms: 10,
                stdout: String::new(),
                stderr: String::new(),
                error: None,
            },
            FanResult {
                repo: "c".into(),
                path: PathBuf::from("/c"),
                argv: vec!["sleep".into()],
                exit_code: None,
                duration_ms: 5000,
                stdout: String::new(),
                stderr: String::new(),
                error: Some("timed out after 5s".into()),
            },
        ];
        let report = FanReport::from_results(vec!["test".into()], results, 5000);
        assert_eq!(report.passed, 1);
        assert_eq!(report.failed, 2);
        assert_eq!(report.timed_out, 1);
    }

    #[test]
    fn fan_out_runs_command_in_repo_dirs() {
        // Use /tmp as a valid directory
        let repos = vec![meta("tmp", "/tmp")];
        let config = FanConfig {
            command: vec!["echo".into(), "hello".into()],
            concurrency: 1,
            timeout: Some(Duration::from_secs(5)),
            fail_fast: false,
        };
        let report = fan_out(&repos, &config);
        assert_eq!(report.results.len(), 1);
        assert!(report.results[0].passed());
        assert!(report.results[0].stdout.trim().contains("hello"));
    }

    #[test]
    fn fan_out_captures_failures() {
        let repos = vec![meta("tmp", "/tmp")];
        let config = FanConfig {
            command: vec!["false".into()],
            concurrency: 1,
            timeout: None,
            fail_fast: false,
        };
        let report = fan_out(&repos, &config);
        assert!(!report.results[0].passed());
        assert_eq!(report.results[0].exit_code, Some(1));
    }

    #[test]
    fn render_grouped_includes_summary() {
        let report = FanReport {
            command: vec!["git".into(), "status".into()],
            results: vec![FanResult {
                repo: "test".into(),
                path: PathBuf::from("/test"),
                argv: vec!["git".into(), "status".into()],
                exit_code: Some(0),
                duration_ms: 100,
                stdout: String::new(),
                stderr: String::new(),
                error: None,
            }],
            passed: 1,
            failed: 0,
            timed_out: 0,
            total_ms: 100,
        };
        let out = render_grouped(&report);
        assert!(out.contains("1 repos"));
        assert!(out.contains("1 passed"));
        assert!(out.contains("0 failed"));
    }

    #[test]
    fn empty_command_produces_error() {
        let repos = vec![meta("tmp", "/tmp")];
        let config = FanConfig {
            command: vec![],
            concurrency: 1,
            timeout: None,
            fail_fast: false,
        };
        let report = fan_out(&repos, &config);
        assert!(report.results[0].error.is_some());
    }
}
