use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

const RUST_SCRIPT_SHEBANGS: &[&str] = &["#!/usr/bin/env rust-script", "#!/usr/bin/rust-script"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRequest {
    pub source: String,
    pub install_dir: PathBuf,
    pub registry_path: PathBuf,
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
            Ok(contents) => {
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
                    runtime: Runtime::RustScript,
                });
            }
            Err(_) => skipped.push(path.display().to_string()),
        }
    }

    if installed.is_empty() {
        bail!(
            "no rust-script shebang files found under {}",
            source_dir.display()
        );
    }

    Ok(InstallReport { installed, skipped })
}

fn install_local_file(source_file: &Path, install_dir: &Path) -> Result<InstalledScript> {
    let contents = read_and_validate_script(source_file)?;
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
        runtime: Runtime::RustScript,
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

    validate_script_contents(&contents, source_url)?;
    let name = script_name_from_url(source_url)?;
    let destination = install_contents(&name, &contents, install_dir, source_url)?;

    Ok(InstalledScript {
        name,
        source: source_url.to_string(),
        destination,
        runtime: Runtime::RustScript,
    })
}

fn read_and_validate_script(path: &Path) -> Result<String> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("reading script {}", path.display()))?;
    validate_script_contents(&contents, &path.display().to_string())?;
    Ok(contents)
}

fn validate_script_contents(contents: &str, label: &str) -> Result<()> {
    let first_line = contents
        .lines()
        .next()
        .ok_or_else(|| anyhow!("script is empty: {label}"))?;

    if RUST_SCRIPT_SHEBANGS.contains(&first_line) {
        return Ok(());
    }

    bail!("unsupported script type for {label}: expected rust-script shebang");
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
    fn rejects_non_rust_script_file() {
        let source_dir = tempdir().expect("tempdir");
        let install_dir = tempdir().expect("tempdir");
        let registry_dir = tempdir().expect("tempdir");
        let script_path = source_dir.path().join("bad.py");
        fs::write(&script_path, "#!/usr/bin/env python3\nprint('hi')\n").expect("write script");

        let error = install(&InstallRequest {
            source: script_path.display().to_string(),
            install_dir: install_dir.path().to_path_buf(),
            registry_path: registry_dir.path().join("registry.json"),
        })
        .expect_err("non-rust-script file should fail");

        assert!(error.to_string().contains("expected rust-script shebang"));
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
}
