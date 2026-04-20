use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    path::{Path, PathBuf},
};

const RUST_SCRIPT_SHEBANGS: &[&str] = &["#!/usr/bin/env rust-script", "#!/usr/bin/rust-script"];
const PYTHON_SHEBANGS: &[&str] = &[
    "#!/usr/bin/env python",
    "#!/usr/bin/env python3",
    "#!/usr/bin/python",
    "#!/usr/bin/python3",
];
const JS_TS_SHEBANGS: &[&str] = &[
    "#!/usr/bin/env node",
    "#!/usr/bin/node",
    "#!/usr/bin/env bun",
    "#!/usr/bin/bun",
];
const BASH_SHEBANGS: &[&str] = &["#!/usr/bin/env bash", "#!/bin/bash"];
const ZSH_SHEBANGS: &[&str] = &["#!/usr/bin/env zsh", "#!/bin/zsh"];
const FISH_SHEBANGS: &[&str] = &["#!/usr/bin/env fish", "#!/usr/bin/fish"];
const NUSHELL_SHEBANGS: &[&str] = &["#!/usr/bin/env nu", "#!/usr/bin/nu"];
const RUBY_SHEBANGS: &[&str] = &["#!/usr/bin/env ruby", "#!/usr/bin/ruby"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRequest {
    pub source: String,
    pub install_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRequest {
    pub name: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectRunRequest {
    pub script_path: PathBuf,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledScript {
    pub name: String,
    pub source: String,
    pub destination: PathBuf,
    pub runtime: Runtime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReport {
    pub installed: Vec<InstalledScript>,
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Runtime {
    #[serde(rename = "rs")]
    RustScript,
    #[serde(rename = "py")]
    Python,
    #[serde(rename = "js")]
    JavaScript,
    #[serde(rename = "ts")]
    TypeScript,
    #[serde(rename = "sh")]
    Bash,
    #[serde(rename = "zsh")]
    Zsh,
    #[serde(rename = "fish")]
    Fish,
    #[serde(rename = "nu")]
    Nushell,
    #[serde(rename = "rb")]
    Ruby,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub name: String,
    pub source: String,
    pub install_path: PathBuf,
    pub runtime: Runtime,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPlan {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CommandPrefixConfig {
    #[serde(default)]
    pub mappings: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub candidate_prefixes: Vec<Vec<String>>,
    #[serde(default)]
    pub learn_on_successful_fallback: bool,
}

// --- Ports ---

pub trait RegistryStore {
    fn list(&self) -> Result<Vec<RegistryEntry>>;
    fn upsert(&mut self, installed: &[InstalledScript]) -> Result<()>;
}

pub trait RemoteScriptFetcher {
    fn fetch(&self, url: &str) -> Result<String>;
}

/// Port for reading a script's source text from an arbitrary location.
pub trait ScriptReader {
    fn read(&self, path: &Path) -> Result<String>;
}

/// Port for writing an installed script to the install directory.
pub trait ScriptWriter {
    fn write(&self, name: &str, contents: &str, install_dir: &Path) -> Result<PathBuf>;
}

/// Port for listing the files inside a directory (non-recursive or recursive
/// is an adapter-level detail; the domain only cares about the flat list of
/// file paths returned).
pub trait DirectoryScanner {
    fn scan_files(&self, dir: &Path) -> Result<Vec<PathBuf>>;
}

// --- Public API ---

pub fn install<R, F, W, S>(
    request: &InstallRequest,
    registry: &mut R,
    fetcher: &F,
    writer: &W,
    scanner: &S,
) -> Result<InstallReport>
where
    R: RegistryStore,
    F: RemoteScriptFetcher,
    W: ScriptWriter,
    S: DirectoryScanner,
{
    let source = resolve_source(&request.source)?;

    let report = match source {
        ResolvedSource::LocalFile(path) => {
            let installed = install_local_file(&path, &request.install_dir, writer)?;
            InstallReport {
                installed: vec![installed],
                skipped: Vec::new(),
            }
        }
        ResolvedSource::LocalDirectory(path) => {
            install_local_directory(&path, &request.install_dir, writer, scanner)?
        }
        ResolvedSource::RemoteUrl(url) => {
            let installed = install_remote_file(&url, &request.install_dir, fetcher, writer)?;
            InstallReport {
                installed: vec![installed],
                skipped: Vec::new(),
            }
        }
    };

    registry.upsert(&report.installed)?;
    Ok(report)
}

pub fn list_installed<R: RegistryStore>(registry: &R) -> Result<Vec<RegistryEntry>> {
    registry.list()
}

pub fn format_registry_entry(entry: &RegistryEntry) -> String {
    format!(
        "{}\t{}\t{}\t{}",
        entry.name,
        entry.description.as_deref().unwrap_or("-"),
        entry.install_path.display(),
        entry.source
    )
}

pub fn plan_installed_run<R: RegistryStore>(
    request: &RunRequest,
    registry: &R,
) -> Result<ExecutionPlan> {
    let entry = registry
        .list()?
        .into_iter()
        .find(|entry| entry.name == request.name)
        .ok_or_else(|| anyhow!("command not found in registry: {}", request.name))?;
    Ok(build_execution_plan(
        &entry.install_path,
        &entry.runtime,
        &request.args,
    ))
}

pub fn plan_direct_run<SR: ScriptReader>(
    request: &DirectRunRequest,
    reader: &SR,
) -> Result<ExecutionPlan> {
    let contents = reader
        .read(&request.script_path)
        .with_context(|| format!("reading script {}", request.script_path.display()))?;
    let runtime = detect_runtime(&contents, &request.script_path.display().to_string())?;
    Ok(build_execution_plan(
        &request.script_path,
        &runtime,
        &request.args,
    ))
}

pub fn apply_command_prefix(plan: &ExecutionPlan, prefix: &[String]) -> Result<ExecutionPlan> {
    let (program, prefix_args) = prefix
        .split_first()
        .ok_or_else(|| anyhow!("command prefix cannot be empty"))?;

    let mut args = prefix_args.to_vec();
    args.push(plan.program.clone());
    args.extend(plan.args.clone());

    Ok(ExecutionPlan {
        program: program.clone(),
        args,
    })
}

// --- Internal ---

enum ResolvedSource {
    LocalFile(PathBuf),
    LocalDirectory(PathBuf),
    RemoteUrl(String),
}

fn resolve_source(input: &str) -> Result<ResolvedSource> {
    if is_url(input) {
        return Ok(ResolvedSource::RemoteUrl(normalize_url(input)));
    }

    let path = PathBuf::from(input);
    if path.is_dir() {
        return Ok(ResolvedSource::LocalDirectory(path));
    }
    if path.is_file() {
        return Ok(ResolvedSource::LocalFile(path));
    }

    bail!("source does not exist or is unsupported: {input}");
}

fn is_url(input: &str) -> bool {
    input.starts_with("https://") || input.starts_with("http://")
}

fn normalize_url(input: &str) -> String {
    if let Some((prefix, suffix)) = input.split_once("github.com/")
        && (prefix.ends_with("https://") || prefix.ends_with("http://"))
    {
        let parts: Vec<&str> = suffix.split('/').collect();
        if parts.len() >= 5 && parts[2] == "blob" {
            let owner = parts[0];
            let repo = parts[1];
            let branch = parts[3];
            let path = parts[4..].join("/");
            return format!("https://raw.githubusercontent.com/{owner}/{repo}/{branch}/{path}");
        }
    }

    input.to_string()
}

fn install_local_directory<W: ScriptWriter, S: DirectoryScanner>(
    source_dir: &Path,
    install_dir: &Path,
    writer: &W,
    scanner: &S,
) -> Result<InstallReport> {
    let mut installed = Vec::new();
    let mut skipped = Vec::new();

    for path in scanner.scan_files(source_dir)? {
        match read_and_validate_script_contents(&path) {
            Ok((contents, runtime)) => {
                let name = script_name(&path)?;
                let destination = writer.write(&name, &contents, install_dir)?;
                installed.push(InstalledScript {
                    name,
                    source: path.display().to_string(),
                    destination,
                    runtime,
                });
            }
            Err(_) => skipped.push(path.display().to_string()),
        }
    }

    if installed.is_empty() {
        bail!(
            "no compatible script shebang files found under {}",
            source_dir.display()
        );
    }

    Ok(InstallReport { installed, skipped })
}

fn install_local_file<W: ScriptWriter>(
    source_file: &Path,
    install_dir: &Path,
    writer: &W,
) -> Result<InstalledScript> {
    let (contents, runtime) = read_and_validate_script_contents(source_file)?;
    let name = script_name(source_file)?;
    let destination = writer.write(&name, &contents, install_dir)?;

    Ok(InstalledScript {
        name,
        source: source_file.display().to_string(),
        destination,
        runtime,
    })
}

fn install_remote_file<F: RemoteScriptFetcher, W: ScriptWriter>(
    source_url: &str,
    install_dir: &Path,
    fetcher: &F,
    writer: &W,
) -> Result<InstalledScript> {
    let contents = fetcher.fetch(source_url)?;
    let runtime = validate_script_contents(&contents, source_url)?;
    let name = script_name_from_url(source_url)?;
    let destination = writer.write(&name, &contents, install_dir)?;

    Ok(InstalledScript {
        name,
        source: source_url.to_string(),
        destination,
        runtime,
    })
}

/// Reads the script at `path` from disk and validates it. This function is
/// used only by the directory-install path, where the scanner has already
/// produced a `PathBuf`. The actual read goes through `std::fs` here because
/// directory scanning is inherently coupled to the local filesystem; the
/// `ScriptReader` port is reserved for `plan_direct_run` where the caller
/// controls the source.
fn read_and_validate_script_contents(path: &Path) -> Result<(String, Runtime)> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("reading script {}", path.display()))?;
    let runtime = validate_script_contents(&contents, &path.display().to_string())?;
    Ok((contents, runtime))
}

fn validate_script_contents(contents: &str, label: &str) -> Result<Runtime> {
    detect_runtime(contents, label)
}

fn detect_runtime(contents: &str, label: &str) -> Result<Runtime> {
    let first_line = contents
        .lines()
        .next()
        .ok_or_else(|| anyhow!("script is empty: {label}"))?;

    if RUST_SCRIPT_SHEBANGS.contains(&first_line) {
        return Ok(Runtime::RustScript);
    }
    if PYTHON_SHEBANGS.contains(&first_line) {
        return Ok(Runtime::Python);
    }
    if JS_TS_SHEBANGS.contains(&first_line) {
        return Ok(match runtime_from_extension(label) {
            Some(Runtime::TypeScript) => Runtime::TypeScript,
            _ => Runtime::JavaScript,
        });
    }
    if BASH_SHEBANGS.contains(&first_line) {
        return Ok(Runtime::Bash);
    }
    if ZSH_SHEBANGS.contains(&first_line) {
        return Ok(Runtime::Zsh);
    }
    if FISH_SHEBANGS.contains(&first_line) {
        return Ok(Runtime::Fish);
    }
    if NUSHELL_SHEBANGS.contains(&first_line) {
        return Ok(Runtime::Nushell);
    }
    if RUBY_SHEBANGS.contains(&first_line) {
        return Ok(Runtime::Ruby);
    }

    bail!("unsupported script type for {label}: expected a supported script shebang");
}

fn runtime_from_extension(label: &str) -> Option<Runtime> {
    let extension = Path::new(label).extension().and_then(OsStr::to_str)?;

    match extension {
        "ts" | "tsx" | "mts" | "cts" => Some(Runtime::TypeScript),
        "js" | "jsx" | "mjs" | "cjs" => Some(Runtime::JavaScript),
        _ => None,
    }
}

fn script_name(path: &Path) -> Result<String> {
    let name = path
        .file_stem()
        .or_else(|| path.file_name())
        .and_then(OsStr::to_str)
        .ok_or_else(|| anyhow!("could not derive script name from {}", path.display()))?;

    Ok(name.to_string())
}

fn script_name_from_url(url: &str) -> Result<String> {
    let tail = url
        .split('/')
        .next_back()
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default();

    if tail.is_empty() {
        bail!("could not derive script name from {url}");
    }

    if let Some(stem) = Path::new(tail).file_stem().and_then(OsStr::to_str) {
        return Ok(stem.to_string());
    }

    bail!("could not derive script name from {url}")
}

fn build_execution_plan(script_path: &Path, runtime: &Runtime, args: &[String]) -> ExecutionPlan {
    let script = script_path.display().to_string();
    let (program, prefix_args): (&str, Vec<String>) = match runtime {
        Runtime::RustScript => ("rust-script", vec![script]),
        Runtime::Python => ("uv", vec!["run".to_string(), script]),
        Runtime::JavaScript | Runtime::TypeScript => ("bun", vec![script]),
        Runtime::Bash => ("bash", vec![script]),
        Runtime::Zsh => ("zsh", vec![script]),
        Runtime::Fish => ("fish", vec![script]),
        Runtime::Nushell => ("nu", vec![script]),
        Runtime::Ruby => ("ruby", vec![script]),
    };

    let mut invocation_args = prefix_args;
    invocation_args.extend(args.iter().cloned());

    ExecutionPlan {
        program: program.to_string(),
        args: invocation_args,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::BTreeMap, fs};
    use tempfile::tempdir;

    // --- Test doubles ---

    #[derive(Default)]
    struct InMemoryRegistry {
        commands: RefCell<Vec<RegistryEntry>>,
    }

    impl RegistryStore for InMemoryRegistry {
        fn list(&self) -> Result<Vec<RegistryEntry>> {
            Ok(self.commands.borrow().clone())
        }

        fn upsert(&mut self, installed: &[InstalledScript]) -> Result<()> {
            let mut entries = self.commands.borrow().clone();
            for script in installed {
                let entry = RegistryEntry {
                    name: script.name.clone(),
                    source: script.source.clone(),
                    install_path: script.destination.clone(),
                    runtime: script.runtime.clone(),
                    description: None,
                };

                if let Some(existing) = entries
                    .iter_mut()
                    .find(|existing| existing.name == entry.name)
                {
                    *existing = entry;
                } else {
                    entries.push(entry);
                }
            }
            entries.sort_by(|left, right| left.name.cmp(&right.name));
            *self.commands.borrow_mut() = entries;
            Ok(())
        }
    }

    #[derive(Default)]
    struct MockFetcher {
        responses: BTreeMap<String, String>,
    }

    impl RemoteScriptFetcher for MockFetcher {
        fn fetch(&self, url: &str) -> Result<String> {
            self.responses
                .get(url)
                .cloned()
                .ok_or_else(|| anyhow!("missing mock response for {url}"))
        }
    }

    /// ScriptWriter that writes to a real tempdir (used where the test needs to
    /// verify the destination path exists on disk).
    struct FsWriter;

    impl ScriptWriter for FsWriter {
        fn write(&self, name: &str, contents: &str, install_dir: &Path) -> Result<PathBuf> {
            let destination = install_dir.join(name);
            fs::write(&destination, contents)?;
            Ok(destination)
        }
    }

    /// ScriptWriter that records calls without touching the filesystem.
    #[derive(Default)]
    struct RecordingWriter {
        written: RefCell<Vec<(String, PathBuf)>>,
    }

    impl ScriptWriter for RecordingWriter {
        fn write(&self, name: &str, _contents: &str, install_dir: &Path) -> Result<PathBuf> {
            let dest = install_dir.join(name);
            self.written.borrow_mut().push((name.to_string(), dest.clone()));
            Ok(dest)
        }
    }

    /// DirectoryScanner backed by the real filesystem using walkdir semantics.
    struct FsScanner;

    impl DirectoryScanner for FsScanner {
        fn scan_files(&self, dir: &Path) -> Result<Vec<PathBuf>> {
            let mut paths = Vec::new();
            for entry in walkdir::WalkDir::new(dir) {
                let entry = entry.with_context(|| format!("walking {}", dir.display()))?;
                if entry.file_type().is_file() {
                    paths.push(entry.into_path());
                }
            }
            Ok(paths)
        }
    }

    /// ScriptReader backed by an in-memory map.
    struct MockReader {
        contents: BTreeMap<PathBuf, String>,
    }

    impl ScriptReader for MockReader {
        fn read(&self, path: &Path) -> Result<String> {
            self.contents
                .get(path)
                .cloned()
                .ok_or_else(|| anyhow!("no mock content for {}", path.display()))
        }
    }

    // --- Tests ---

    #[test]
    fn installs_single_local_rust_script() -> Result<()> {
        let source_dir = tempdir()?;
        let install_dir = tempdir()?;
        let script_path = source_dir.path().join("hello.rs");
        fs::write(&script_path, "#!/usr/bin/env rust-script\nfn main() {}\n")?;

        let mut registry = InMemoryRegistry::default();
        let fetcher = MockFetcher::default();
        let report = install(
            &InstallRequest {
                source: script_path.display().to_string(),
                install_dir: install_dir.path().to_path_buf(),
            },
            &mut registry,
            &fetcher,
            &FsWriter,
            &FsScanner,
        )?;

        assert_eq!(report.installed.len(), 1);
        assert!(report.skipped.is_empty());
        assert_eq!(
            report.installed[0].destination,
            install_dir.path().join("hello")
        );
        assert_eq!(report.installed[0].name, "hello");
        assert_eq!(report.installed[0].runtime, Runtime::RustScript);
        Ok(())
    }

    #[test]
    fn installs_compatible_scripts_from_directory() -> Result<()> {
        let source_dir = tempdir()?;
        let install_dir = tempdir()?;
        fs::write(
            source_dir.path().join("good.rs"),
            "#!/usr/bin/env rust-script\nfn main() {}\n",
        )?;
        fs::write(
            source_dir.path().join("good.py"),
            "#!/usr/bin/env python3\nprint('hi')\n",
        )?;
        fs::write(source_dir.path().join("bad.txt"), "hello\n")?;

        let mut registry = InMemoryRegistry::default();
        let fetcher = MockFetcher::default();
        let report = install(
            &InstallRequest {
                source: source_dir.path().display().to_string(),
                install_dir: install_dir.path().to_path_buf(),
            },
            &mut registry,
            &fetcher,
            &FsWriter,
            &FsScanner,
        )?;

        assert_eq!(report.installed.len(), 2);
        assert_eq!(report.skipped.len(), 1);
        Ok(())
    }

    #[test]
    fn installs_remote_python_script() -> Result<()> {
        let install_dir = tempdir()?;
        let mut registry = InMemoryRegistry::default();
        let fetcher = MockFetcher {
            responses: BTreeMap::from([(
                "https://example.com/demo.py".to_string(),
                "#!/usr/bin/env python3\nprint('hi')\n".to_string(),
            )]),
        };

        let report = install(
            &InstallRequest {
                source: "https://example.com/demo.py".to_string(),
                install_dir: install_dir.path().to_path_buf(),
            },
            &mut registry,
            &fetcher,
            &FsWriter,
            &FsScanner,
        )?;

        assert_eq!(report.installed[0].runtime, Runtime::Python);
        Ok(())
    }

    #[test]
    fn rejects_unsupported_script_file() {
        let source_dir = tempdir().expect("tempdir");
        let install_dir = tempdir().expect("tempdir");
        let script_path = source_dir.path().join("bad.txt");
        fs::write(&script_path, "hello\n").expect("write script");

        let mut registry = InMemoryRegistry::default();
        let fetcher = MockFetcher::default();
        let error = install(
            &InstallRequest {
                source: script_path.display().to_string(),
                install_dir: install_dir.path().to_path_buf(),
            },
            &mut registry,
            &fetcher,
            &FsWriter,
            &FsScanner,
        )
        .expect_err("unsupported file should fail");

        assert!(
            error
                .to_string()
                .contains("expected a supported script shebang")
        );
    }

    #[test]
    fn normalizes_github_blob_urls() {
        let normalized =
            normalize_url("https://github.com/example/tools/blob/main/scripts/preflight.rs");

        assert_eq!(
            normalized,
            "https://raw.githubusercontent.com/example/tools/main/scripts/preflight.rs"
        );
    }

    #[test]
    fn list_installed_returns_registry_entries() -> Result<()> {
        let mut registry = InMemoryRegistry::default();
        registry.upsert(&[InstalledScript {
            name: "hello".to_string(),
            source: "https://example.com/hello.rs".to_string(),
            destination: PathBuf::from("/tmp/rx/bin/hello"),
            runtime: Runtime::RustScript,
        }])?;

        let commands = list_installed(&registry)?;
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "hello");
        Ok(())
    }

    #[test]
    fn format_registry_entry_includes_all_expected_columns() {
        let entry = RegistryEntry {
            name: "hello".to_string(),
            source: "https://example.com/hello.rs".to_string(),
            install_path: PathBuf::from("/tmp/rx/bin/hello"),
            runtime: Runtime::RustScript,
            description: None,
        };

        assert_eq!(
            format_registry_entry(&entry),
            "hello\t-\t/tmp/rx/bin/hello\thttps://example.com/hello.rs"
        );
    }

    #[test]
    fn plan_installed_run_uses_registry_install_path() -> Result<()> {
        let mut registry = InMemoryRegistry::default();
        registry.upsert(&[InstalledScript {
            name: "hello".to_string(),
            source: "https://example.com/hello.py".to_string(),
            destination: PathBuf::from("/tmp/rx/bin/hello"),
            runtime: Runtime::Python,
        }])?;

        let plan = plan_installed_run(
            &RunRequest {
                name: "hello".to_string(),
                args: vec!["--flag".to_string()],
            },
            &registry,
        )?;

        assert_eq!(plan.program, "uv");
        assert_eq!(
            plan.args,
            vec![
                "run".to_string(),
                "/tmp/rx/bin/hello".to_string(),
                "--flag".to_string()
            ]
        );
        Ok(())
    }

    #[test]
    fn plan_direct_run_uses_rust_script_runtime() -> Result<()> {
        let script_path = PathBuf::from("/tmp/hello.rs");
        let reader = MockReader {
            contents: BTreeMap::from([(
                script_path.clone(),
                "#!/usr/bin/env rust-script\nfn main() {}\n".to_string(),
            )]),
        };

        let plan = plan_direct_run(
            &DirectRunRequest {
                script_path: script_path.clone(),
                args: vec!["--demo".to_string()],
            },
            &reader,
        )?;

        assert_eq!(plan.program, "rust-script");
        assert_eq!(
            plan.args,
            vec![script_path.display().to_string(), "--demo".to_string()]
        );
        Ok(())
    }

    #[test]
    fn detect_runtime_supports_python_js_ts_shells_and_ruby() -> Result<()> {
        let cases = [
            (
                "#!/usr/bin/env python3\nprint('hi')\n",
                "demo.py",
                Runtime::Python,
            ),
            (
                "#!/usr/bin/env node\nconsole.log('hi')\n",
                "demo.js",
                Runtime::JavaScript,
            ),
            (
                "#!/usr/bin/env bun\nconsole.log('hi')\n",
                "demo.ts",
                Runtime::TypeScript,
            ),
            ("#!/usr/bin/env bash\necho hi\n", "demo.sh", Runtime::Bash),
            ("#!/usr/bin/env zsh\necho hi\n", "demo.zsh", Runtime::Zsh),
            ("#!/usr/bin/env fish\necho hi\n", "demo.fish", Runtime::Fish),
            (
                "#!/usr/bin/env nu\nprint 'hi'\n",
                "demo.nu",
                Runtime::Nushell,
            ),
            ("#!/usr/bin/env ruby\nputs 'hi'\n", "demo.rb", Runtime::Ruby),
        ];

        for (contents, label, expected) in cases {
            assert_eq!(detect_runtime(contents, label)?, expected);
        }

        Ok(())
    }

    #[test]
    fn build_execution_plan_uses_expected_launchers() {
        let cases = [
            (
                Runtime::RustScript,
                "/tmp/demo.rs",
                "rust-script",
                vec!["/tmp/demo.rs".to_string(), "--flag".to_string()],
            ),
            (
                Runtime::Python,
                "/tmp/demo.py",
                "uv",
                vec![
                    "run".to_string(),
                    "/tmp/demo.py".to_string(),
                    "--flag".to_string(),
                ],
            ),
            (
                Runtime::JavaScript,
                "/tmp/demo.js",
                "bun",
                vec!["/tmp/demo.js".to_string(), "--flag".to_string()],
            ),
            (
                Runtime::TypeScript,
                "/tmp/demo.ts",
                "bun",
                vec!["/tmp/demo.ts".to_string(), "--flag".to_string()],
            ),
            (
                Runtime::Bash,
                "/tmp/demo.sh",
                "bash",
                vec!["/tmp/demo.sh".to_string(), "--flag".to_string()],
            ),
            (
                Runtime::Zsh,
                "/tmp/demo.zsh",
                "zsh",
                vec!["/tmp/demo.zsh".to_string(), "--flag".to_string()],
            ),
            (
                Runtime::Fish,
                "/tmp/demo.fish",
                "fish",
                vec!["/tmp/demo.fish".to_string(), "--flag".to_string()],
            ),
            (
                Runtime::Nushell,
                "/tmp/demo.nu",
                "nu",
                vec!["/tmp/demo.nu".to_string(), "--flag".to_string()],
            ),
            (
                Runtime::Ruby,
                "/tmp/demo.rb",
                "ruby",
                vec!["/tmp/demo.rb".to_string(), "--flag".to_string()],
            ),
        ];

        for (runtime, script_path, program, args) in cases {
            let plan =
                build_execution_plan(Path::new(script_path), &runtime, &["--flag".to_string()]);
            assert_eq!(plan.program, program);
            assert_eq!(plan.args, args);
        }
    }

    #[test]
    fn apply_command_prefix_wraps_existing_plan() -> Result<()> {
        let plan = ExecutionPlan {
            program: "gh".to_string(),
            args: vec!["issue".to_string(), "list".to_string()],
        };

        let prefixed = apply_command_prefix(
            &plan,
            &[
                "op".to_string(),
                "plugin".to_string(),
                "run".to_string(),
                "--".to_string(),
            ],
        )?;

        assert_eq!(prefixed.program, "op");
        assert_eq!(
            prefixed.args,
            vec![
                "plugin".to_string(),
                "run".to_string(),
                "--".to_string(),
                "gh".to_string(),
                "issue".to_string(),
                "list".to_string(),
            ]
        );
        Ok(())
    }

    #[test]
    fn apply_command_prefix_rejects_empty_prefix() {
        let plan = ExecutionPlan {
            program: "gh".to_string(),
            args: Vec::new(),
        };

        let error = apply_command_prefix(&plan, &[]).expect_err("empty prefix should fail");
        assert!(error.to_string().contains("command prefix cannot be empty"));
    }

    #[test]
    fn runtime_serializes_to_extension_style_codes() -> Result<()> {
        let cases = [
            (Runtime::RustScript, "\"rs\""),
            (Runtime::Python, "\"py\""),
            (Runtime::JavaScript, "\"js\""),
            (Runtime::TypeScript, "\"ts\""),
            (Runtime::Bash, "\"sh\""),
            (Runtime::Zsh, "\"zsh\""),
            (Runtime::Fish, "\"fish\""),
            (Runtime::Nushell, "\"nu\""),
            (Runtime::Ruby, "\"rb\""),
        ];

        for (runtime, expected) in cases {
            assert_eq!(serde_json::to_string(&runtime)?, expected);
        }

        Ok(())
    }

    #[test]
    fn install_uses_recording_writer_without_touching_disk() -> Result<()> {
        let source_dir = tempdir()?;
        let install_dir = tempdir()?;
        let script_path = source_dir.path().join("hello.rs");
        fs::write(&script_path, "#!/usr/bin/env rust-script\nfn main() {}\n")?;

        let mut registry = InMemoryRegistry::default();
        let writer = RecordingWriter::default();
        let report = install(
            &InstallRequest {
                source: script_path.display().to_string(),
                install_dir: install_dir.path().to_path_buf(),
            },
            &mut registry,
            &MockFetcher::default(),
            &writer,
            &FsScanner,
        )?;

        assert_eq!(report.installed.len(), 1);
        assert_eq!(writer.written.borrow().len(), 1);
        assert_eq!(writer.written.borrow()[0].0, "hello");
        Ok(())
    }

    #[test]
    fn plan_direct_run_uses_mock_reader_without_disk() -> Result<()> {
        let script_path = PathBuf::from("/nonexistent/hello.py");
        let reader = MockReader {
            contents: BTreeMap::from([(
                script_path.clone(),
                "#!/usr/bin/env python3\nprint('hi')\n".to_string(),
            )]),
        };

        let plan = plan_direct_run(
            &DirectRunRequest {
                script_path,
                args: Vec::new(),
            },
            &reader,
        )?;

        assert_eq!(plan.program, "uv");
        Ok(())
    }
}
