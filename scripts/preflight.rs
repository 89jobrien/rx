#!/usr/bin/env rust-script
//! ```cargo
//! [dependencies]
//! anyhow   = "1"
//! colored  = "2"
//! chrono   = { version = "0.4", features = ["clock"] }
//! ```

use anyhow::{Context, Result};
use colored::Colorize;
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

trait CommandRunner {
    fn capture(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Option<String>;
}

struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn capture(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Option<String> {
        let mut command = Command::new(cmd);
        command.args(args);

        if let Some(dir) = cwd {
            command.current_dir(dir);
        }

        command
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

struct Console;

impl Console {
    fn header(&self, title: &str) {
        println!("{}", title.bold().yellow());
    }

    fn section(&self, title: &str) {
        println!("\n{}", format!("── {title} ").bold().cyan());
    }
}

struct EnvironmentInspector<'a, R> {
    runner: &'a R,
}

impl<'a, R: CommandRunner> EnvironmentInspector<'a, R> {
    fn detect_shell(&self) -> String {
        env::var("SHELL")
            .ok()
            .and_then(|shell| {
                Path::new(&shell)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .or_else(|| {
                let ppid = self.runner.capture("sh", &["-c", "echo $PPID"], None)?;
                self.runner
                    .capture("ps", &["-p", &ppid, "-o", "comm="], None)
            })
            .unwrap_or_else(|| "unknown".to_string())
    }
}

struct GitClient<'a, R> {
    runner: &'a R,
    root: PathBuf,
}

impl<'a, R: CommandRunner> GitClient<'a, R> {
    fn discover(runner: &'a R, cwd: &Path) -> Self {
        let root = runner
            .capture("git", &["rev-parse", "--show-toplevel"], Some(cwd))
            .map(PathBuf::from)
            .unwrap_or_else(|| cwd.to_path_buf());

        Self { runner, root }
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn branch(&self) -> String {
        self.runner
            .capture("git", &["branch", "--show-current"], Some(&self.root))
            .unwrap_or_else(|| "(detached)".to_string())
    }

    fn status_short(&self) -> String {
        self.runner
            .capture("git", &["status", "--short"], Some(&self.root))
            .unwrap_or_default()
    }

    fn recent_history(&self, count: usize) -> String {
        let depth = format!("-{count}");

        self.runner
            .capture(
                "git",
                &["log", "--oneline", &depth, "--decorate"],
                Some(&self.root),
            )
            .unwrap_or_else(|| "(no commits)".to_string())
    }

    fn tracked_tree(&self, max_depth: usize) -> String {
        let files = self
            .runner
            .capture("git", &["ls-files"], Some(&self.root))
            .unwrap_or_default();

        let mut visible_paths = Vec::new();
        for path in files.lines() {
            let parts: Vec<String> = path.split('/').map(String::from).collect();
            if parts.len() <= max_depth + 1 {
                visible_paths.push(parts);
            }
        }

        let mut seen = BTreeSet::new();
        let mut lines = Vec::new();

        for parts in &visible_paths {
            for depth in 0..parts.len() {
                let key = parts[..=depth].join("/");
                if seen.insert(key) {
                    let indent = "  ".repeat(depth);
                    let is_leaf = depth == parts.len() - 1;
                    let label = if is_leaf {
                        parts[depth].normal().to_string()
                    } else {
                        parts[depth].bold().blue().to_string()
                    };

                    lines.push(format!("{indent}{label}"));
                }
            }
        }

        if lines.is_empty() {
            "(no tracked files)".to_string()
        } else {
            lines.join("\n")
        }
    }
}

struct HandoffService {
    root: PathBuf,
}

impl HandoffService {
    fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn find_files(&self) -> Vec<PathBuf> {
        let mut hits = Vec::new();

        if let Ok(entries) = fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("HANDOFF") {
                    hits.push(entry.path());
                }
            }
        }

        hits.sort();
        hits
    }

    fn read(&self, path: &Path) -> Result<String> {
        fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
    }

    fn stamp(&self, path: &Path) -> Result<bool> {
        let now = chrono::Local::now()
            .format("%Y-%m-%d %H:%M:%S %z")
            .to_string();
        let original =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;

        let stamp_line = format!("last_preflight: {now}");
        let updated = if let Some(pos) = original.find("last_preflight:") {
            let end = original[pos..]
                .find('\n')
                .map(|index| pos + index + 1)
                .unwrap_or(original.len());
            format!("{}{stamp_line}\n{}", &original[..pos], &original[end..])
        } else {
            format!("{stamp_line}\n{original}")
        };

        if updated == original {
            return Ok(false);
        }

        fs::write(path, &updated).with_context(|| format!("writing {}", path.display()))?;
        Ok(true)
    }
}

struct PreflightApp<'a, R> {
    console: Console,
    environment: EnvironmentInspector<'a, R>,
    git: GitClient<'a, R>,
    handoffs: HandoffService,
}

impl<'a, R: CommandRunner> PreflightApp<'a, R> {
    fn new(runner: &'a R, cwd: &Path) -> Self {
        let git = GitClient::discover(runner, cwd);
        let handoffs = HandoffService::new(git.root());

        Self {
            console: Console,
            environment: EnvironmentInspector { runner },
            git,
            handoffs,
        }
    }

    fn run(&self, cwd: &Path) -> Result<()> {
        self.console.header("╔══ preflight ══╗");

        self.print_environment(cwd);
        self.print_git_status();
        self.print_git_history();
        self.print_tracked_files();
        self.print_handoffs()?;

        self.console.header("╚══ done ══╝");
        Ok(())
    }

    fn print_environment(&self, cwd: &Path) {
        self.console.section("Environment");
        println!("  shell    : {}", self.environment.detect_shell().green());
        println!("  cwd      : {}", cwd.display().to_string().green());
        println!(
            "  git root : {}",
            self.git.root().display().to_string().dimmed()
        );
    }

    fn print_git_status(&self) {
        self.console.section("Git — branch / status");
        println!("  branch   : {}", self.git.branch().yellow());

        let status = self.git.status_short();
        if status.is_empty() {
            println!("  status   : {}", "clean".green());
            return;
        }

        for line in status.lines() {
            println!("  {line}");
        }
    }

    fn print_git_history(&self) {
        self.console.section("Git — recent history");
        for line in self.git.recent_history(7).lines() {
            println!("  {line}");
        }
    }

    fn print_tracked_files(&self) {
        self.console.section("Tracked files (depth ≤ 3)");
        println!("{}", self.git.tracked_tree(3));
    }

    fn print_handoffs(&self) -> Result<()> {
        self.console.section("HANDOFF documents");

        let handoffs = self.handoffs.find_files();
        if handoffs.is_empty() {
            println!("  (none found)");
            return Ok(());
        }

        for path in &handoffs {
            println!("\n  {}", path.display().to_string().bold());

            match self.handoffs.read(path) {
                Ok(contents) => {
                    for line in contents.lines().take(40) {
                        println!("  {line}");
                    }

                    let total_lines = contents.lines().count();
                    if total_lines > 40 {
                        println!("  {} … ({} more lines)", "↳".dimmed(), total_lines - 40);
                    }

                    if self.handoffs.stamp(path)? {
                        println!(
                            "  {} updated timestamp",
                            path.display().to_string().dimmed()
                        );
                    }
                }
                Err(error) => println!("  {}", format!("error: {error}").red()),
            }
        }

        Ok(())
    }
}

fn main() -> Result<()> {
    let cwd = env::current_dir()?;
    let runner = SystemCommandRunner;
    let app = PreflightApp::new(&runner, &cwd);
    app.run(&cwd)
}
