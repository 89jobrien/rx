use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
};
use walkdir::WalkDir;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRequest {
    pub source: String,
    pub install_dir: PathBuf,
    pub registry_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRequest {
    pub name: String,
    pub registry_path: PathBuf,
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
#[serde(rename_all = "kebab-case")]
pub enum Runtime {
    RustScript,
    Python,
    JavaScript,
    TypeScript,
    Bash,
    Zsh,
    Fish,
    Nushell,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub name: String,
    pub source: String,
    pub install_path: PathBuf,
    pub runtime: Runtime,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RegistryFile {
    version: u32,
    commands: Vec<RegistryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPlan {
    pub program: String,
    pub args: Vec<String>,
}

pub fn install(request: &InstallRequest) -> Result<InstallReport> {
    let source = resolve_source(&request.source)?;
    fs::create_dir_all(&request.install_dir).with_context(|| {
        format!(
            "creating install directory {}",
            request.install_dir.display()
        )
    })?;
    ensure_registry_parent(&request.registry_path)?;

    let report = match source {
        ResolvedSource::LocalFile(path) => {
            let installed = install_local_file(&path, &request.install_dir)?;
            Ok(InstallReport {
                installed: vec![installed],
                skipped: Vec::new(),
            })
        }
        ResolvedSource::LocalDirectory(path) => {
            install_local_directory(&path, &request.install_dir)
        }
        ResolvedSource::RemoteUrl(url) => {
            let installed = install_remote_file(&url, &request.install_dir)?;
            Ok(InstallReport {
                installed: vec![installed],
                skipped: Vec::new(),
            })
        }
    }?;

    update_registry(&request.registry_path, &report.installed)?;
    Ok(report)
}

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

fn install_local_directory(source_dir: &Path, install_dir: &Path) -> Result<InstallReport> {
    let mut installed = Vec::new();
    let mut skipped = Vec::new();

    for entry in WalkDir::new(source_dir) {
        let entry = entry.with_context(|| format!("walking {}", source_dir.display()))?;
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.into_path();
        match read_and_validate_script(&path) {
            Ok((contents, runtime)) => {
                let destination = install_contents(
                    &script_name(&path)?,
                    &contents,
                    install_dir,
                    &path.display().to_string(),
                )?;
                let name = script_name(&path)?;
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

fn install_local_file(source_file: &Path, install_dir: &Path) -> Result<InstalledScript> {
    let (contents, runtime) = read_and_validate_script(source_file)?;
    let name = script_name(source_file)?;
    let destination = install_contents(
        &name,
        &contents,
        install_dir,
        &source_file.display().to_string(),
    )?;

    Ok(InstalledScript {
        name,
        source: source_file.display().to_string(),
        destination,
        runtime,
    })
}

fn install_remote_file(source_url: &str, install_dir: &Path) -> Result<InstalledScript> {
    let response = reqwest::blocking::get(source_url)
        .with_context(|| format!("downloading {source_url}"))?
        .error_for_status()
        .with_context(|| format!("downloading {source_url}"))?;

    let contents = response
        .text()
        .with_context(|| format!("reading response body from {source_url}"))?;

    let runtime = validate_script_contents(&contents, source_url)?;
    let name = script_name_from_url(source_url)?;
    let destination = install_contents(&name, &contents, install_dir, source_url)?;

    Ok(InstalledScript {
        name,
        source: source_url.to_string(),
        destination,
        runtime,
    })
}

fn read_and_validate_script(path: &Path) -> Result<(String, Runtime)> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("reading script {}", path.display()))?;
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

fn install_contents(
    name: &str,
    contents: &str,
    install_dir: &Path,
    source: &str,
) -> Result<PathBuf> {
    let destination = install_dir.join(name);
    fs::write(&destination, contents)
        .with_context(|| format!("writing {} from {source}", destination.display()))?;
    make_executable(&destination)?;
    Ok(destination)
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

pub fn default_paths() -> Result<RxPaths> {
    let root = rx_home_dir()?;
    Ok(RxPaths {
        root: root.clone(),
        bin_dir: root.join("bin"),
        registry_path: root.join("registry.json"),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RxPaths {
    pub root: PathBuf,
    pub bin_dir: PathBuf,
    pub registry_path: PathBuf,
}

pub fn list_installed(registry_path: &Path) -> Result<Vec<RegistryEntry>> {
    Ok(load_registry(registry_path)?.commands)
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

pub fn run_installed(request: &RunRequest) -> Result<ExitStatus> {
    let entry = list_installed(&request.registry_path)?
        .into_iter()
        .find(|entry| entry.name == request.name)
        .ok_or_else(|| anyhow!("command not found in registry: {}", request.name))?;
    let plan = build_execution_plan(&entry.install_path, &entry.runtime, &request.args);
    execute_plan(&plan)
}

pub fn run_direct(request: &DirectRunRequest) -> Result<ExitStatus> {
    let plan = plan_direct_execution(&request.script_path, &request.args)?;
    execute_plan(&plan)
}

fn rx_home_dir() -> Result<PathBuf> {
    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(config_home).join("rx"));
    }

    if let Some(home) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(home).join(".config").join("rx"));
    }

    bail!("could not determine rx config home from XDG_CONFIG_HOME or HOME")
}

fn ensure_registry_parent(registry_path: &Path) -> Result<()> {
    let parent = registry_path.parent().ok_or_else(|| {
        anyhow!(
            "registry path has no parent directory: {}",
            registry_path.display()
        )
    })?;

    fs::create_dir_all(parent)
        .with_context(|| format!("creating registry directory {}", parent.display()))?;
    Ok(())
}

fn update_registry(registry_path: &Path, installed: &[InstalledScript]) -> Result<()> {
    let mut registry = load_registry(registry_path)?;

    for script in installed {
        let entry = RegistryEntry {
            name: script.name.clone(),
            source: script.source.clone(),
            install_path: script.destination.clone(),
            runtime: script.runtime.clone(),
            description: None,
        };

        if let Some(existing) = registry
            .commands
            .iter_mut()
            .find(|existing| existing.name == entry.name)
        {
            *existing = entry;
        } else {
            registry.commands.push(entry);
        }
    }

    registry
        .commands
        .sort_by(|left, right| left.name.cmp(&right.name));
    save_registry(registry_path, &registry)
}

fn load_registry(registry_path: &Path) -> Result<RegistryFile> {
    if !registry_path.exists() {
        return Ok(RegistryFile {
            version: 1,
            commands: Vec::new(),
        });
    }

    let contents = fs::read_to_string(registry_path)
        .with_context(|| format!("reading registry {}", registry_path.display()))?;
    let registry: RegistryFile = serde_json::from_str(&contents)
        .with_context(|| format!("parsing registry {}", registry_path.display()))?;
    Ok(registry)
}

fn save_registry(registry_path: &Path, registry: &RegistryFile) -> Result<()> {
    let contents = serde_json::to_string_pretty(registry)
        .with_context(|| format!("serializing registry {}", registry_path.display()))?;
    fs::write(registry_path, contents)
        .with_context(|| format!("writing registry {}", registry_path.display()))?;
    Ok(())
}

fn plan_direct_execution(script_path: &Path, args: &[String]) -> Result<ExecutionPlan> {
    let contents = fs::read_to_string(script_path)
        .with_context(|| format!("reading script {}", script_path.display()))?;
    let runtime = detect_runtime(&contents, &script_path.display().to_string())?;
    Ok(build_execution_plan(script_path, &runtime, args))
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
    };

    let mut invocation_args = prefix_args;
    invocation_args.extend(args.iter().cloned());

    ExecutionPlan {
        program: program.to_string(),
        args: invocation_args,
    }
}

pub fn execute_plan(plan: &ExecutionPlan) -> Result<ExitStatus> {
    Command::new(&plan.program)
        .args(&plan.args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("running {}", plan.program))
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .with_context(|| format!("reading permissions for {}", path.display()))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("setting executable bit on {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn installs_single_local_rust_script() -> Result<()> {
        let source_dir = tempdir()?;
        let install_dir = tempdir()?;
        let registry_dir = tempdir()?;
        let script_path = source_dir.path().join("hello.rs");
        fs::write(&script_path, "#!/usr/bin/env rust-script\nfn main() {}\n")?;

        let report = install(&InstallRequest {
            source: script_path.display().to_string(),
            install_dir: install_dir.path().to_path_buf(),
            registry_path: registry_dir.path().join("registry.json"),
        })?;

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
    fn installs_only_rust_scripts_from_directory() -> Result<()> {
        let source_dir = tempdir()?;
        let install_dir = tempdir()?;
        let registry_dir = tempdir()?;
        fs::write(
            source_dir.path().join("good.rs"),
            "#!/usr/bin/env rust-script\nfn main() {}\n",
        )?;
        fs::write(source_dir.path().join("bad.sh"), "#!/bin/sh\necho hi\n")?;

        let report = install(&InstallRequest {
            source: source_dir.path().display().to_string(),
            install_dir: install_dir.path().to_path_buf(),
            registry_path: registry_dir.path().join("registry.json"),
        })?;

        assert_eq!(report.installed.len(), 1);
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(
            report.installed[0].destination,
            install_dir.path().join("good")
        );
        Ok(())
    }

    #[test]
    fn installs_python_script_with_python_runtime() -> Result<()> {
        let source_dir = tempdir().expect("tempdir");
        let install_dir = tempdir().expect("tempdir");
        let registry_dir = tempdir().expect("tempdir");
        let script_path = source_dir.path().join("hello.py");
        fs::write(&script_path, "#!/usr/bin/env python3\nprint('hi')\n").expect("write script");

        let report = install(&InstallRequest {
            source: script_path.display().to_string(),
            install_dir: install_dir.path().to_path_buf(),
            registry_path: registry_dir.path().join("registry.json"),
        })?;

        assert_eq!(report.installed.len(), 1);
        assert_eq!(report.installed[0].runtime, Runtime::Python);
        Ok(())
    }

    #[test]
    fn rejects_unsupported_script_file() {
        let source_dir = tempdir().expect("tempdir");
        let install_dir = tempdir().expect("tempdir");
        let registry_dir = tempdir().expect("tempdir");
        let script_path = source_dir.path().join("bad.rb");
        fs::write(&script_path, "#!/usr/bin/env ruby\nputs 'hi'\n").expect("write script");

        let error = install(&InstallRequest {
            source: script_path.display().to_string(),
            install_dir: install_dir.path().to_path_buf(),
            registry_path: registry_dir.path().join("registry.json"),
        })
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
    fn writes_registry_entries_for_installed_scripts() -> Result<()> {
        let source_dir = tempdir()?;
        let install_dir = tempdir()?;
        let registry_dir = tempdir()?;
        let registry_path = registry_dir.path().join("registry.json");
        let script_path = source_dir.path().join("hello.rs");
        fs::write(&script_path, "#!/usr/bin/env rust-script\nfn main() {}\n")?;

        install(&InstallRequest {
            source: script_path.display().to_string(),
            install_dir: install_dir.path().to_path_buf(),
            registry_path: registry_path.clone(),
        })?;

        let commands = list_installed(&registry_path)?;
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "hello");
        assert_eq!(commands[0].source, script_path.display().to_string());
        assert_eq!(commands[0].install_path, install_dir.path().join("hello"));
        assert_eq!(commands[0].runtime, Runtime::RustScript);
        assert_eq!(commands[0].description, None);
        Ok(())
    }

    #[test]
    fn reinstall_updates_existing_registry_entry_without_duplicates() -> Result<()> {
        let first_source = tempdir()?;
        let second_source = tempdir()?;
        let install_dir = tempdir()?;
        let registry_dir = tempdir()?;
        let registry_path = registry_dir.path().join("registry.json");
        let first_path = first_source.path().join("hello.rs");
        let second_path = second_source.path().join("hello.rs");
        fs::write(&first_path, "#!/usr/bin/env rust-script\nfn main() {}\n")?;
        fs::write(
            &second_path,
            "#!/usr/bin/env rust-script\nfn main() { println!(\"v2\"); }\n",
        )?;

        install(&InstallRequest {
            source: first_path.display().to_string(),
            install_dir: install_dir.path().to_path_buf(),
            registry_path: registry_path.clone(),
        })?;
        install(&InstallRequest {
            source: second_path.display().to_string(),
            install_dir: install_dir.path().to_path_buf(),
            registry_path: registry_path.clone(),
        })?;

        let commands = list_installed(&registry_path)?;
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].source, second_path.display().to_string());
        Ok(())
    }

    #[test]
    fn list_installed_returns_empty_for_missing_registry() -> Result<()> {
        let registry_dir = tempdir()?;
        let commands = list_installed(&registry_dir.path().join("registry.json"))?;
        assert!(commands.is_empty());
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
    fn build_execution_plan_uses_registry_install_path_for_rust_script() -> Result<()> {
        let source_dir = tempdir()?;
        let install_dir = tempdir()?;
        let registry_dir = tempdir()?;
        let registry_path = registry_dir.path().join("registry.json");
        let script_path = source_dir.path().join("hello.rs");
        fs::write(&script_path, "#!/usr/bin/env rust-script\nfn main() {}\n")?;

        install(&InstallRequest {
            source: script_path.display().to_string(),
            install_dir: install_dir.path().to_path_buf(),
            registry_path: registry_path.clone(),
        })?;

        let entry = list_installed(&registry_path)?
            .into_iter()
            .find(|entry| entry.name == "hello")
            .expect("installed command exists");
        let plan = build_execution_plan(
            &entry.install_path,
            &entry.runtime,
            &["--flag".to_string(), "value".to_string()],
        );

        assert_eq!(plan.program, "rust-script");
        assert_eq!(
            plan.args,
            vec![
                install_dir.path().join("hello").display().to_string(),
                "--flag".to_string(),
                "value".to_string(),
            ]
        );
        Ok(())
    }

    #[test]
    fn plan_direct_execution_uses_rust_script_runtime() -> Result<()> {
        let source_dir = tempdir()?;
        let script_path = source_dir.path().join("hello.rs");
        fs::write(&script_path, "#!/usr/bin/env rust-script\nfn main() {}\n")?;

        let plan = plan_direct_execution(&script_path, &["--demo".to_string()])?;

        assert_eq!(plan.program, "rust-script");
        assert_eq!(
            plan.args,
            vec![script_path.display().to_string(), "--demo".to_string()]
        );
        Ok(())
    }

    #[test]
    fn detect_runtime_supports_python_js_ts_and_shells() -> Result<()> {
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
                "#!/usr/bin/env node\nconsole.log('hi')\n",
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
        ];

        for (contents, label, expected) in cases {
            assert_eq!(detect_runtime(contents, label)?, expected);
        }

        Ok(())
    }

    #[test]
    fn build_execution_plan_uses_expected_launchers_for_supported_runtimes() {
        let cases = [
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
        ];

        for (runtime, script_path, program, args) in cases {
            let plan =
                build_execution_plan(Path::new(script_path), &runtime, &["--flag".to_string()]);
            assert_eq!(plan.program, program);
            assert_eq!(plan.args, args);
        }
    }
}
