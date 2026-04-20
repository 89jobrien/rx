use anyhow::{Context, Result, anyhow};
use rx_script_core::{
    DirectoryScanner, InstalledScript, RegistryEntry, RegistryStore, RemoteScriptFetcher,
    ScriptReader, ScriptWriter,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

// --- Path helpers ---

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RxPaths {
    pub root: PathBuf,
    pub bin_dir: PathBuf,
    pub registry_path: PathBuf,
}

pub fn default_paths() -> Result<RxPaths> {
    let root = rx_home_dir()?;
    Ok(RxPaths {
        root: root.clone(),
        bin_dir: root.join("bin"),
        registry_path: root.join("registry.json"),
    })
}

fn rx_home_dir() -> Result<PathBuf> {
    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(config_home).join("rx"));
    }

    if let Some(home) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(home).join(".config").join("rx"));
    }

    Err(anyhow!(
        "could not determine rx config home from XDG_CONFIG_HOME or HOME"
    ))
}

// --- Registry adapter ---

pub struct JsonRegistryStore {
    path: PathBuf,
}

impl JsonRegistryStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RegistryFile {
    version: u32,
    commands: Vec<RegistryEntry>,
}

impl RegistryStore for JsonRegistryStore {
    fn list(&self) -> Result<Vec<RegistryEntry>> {
        Ok(load_registry(&self.path)?.commands)
    }

    fn upsert(&mut self, installed: &[InstalledScript]) -> Result<()> {
        ensure_registry_parent(&self.path)?;
        let mut registry = load_registry(&self.path)?;

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
        save_registry(&self.path, &registry)
    }
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

// --- HTTP fetcher adapter ---

pub struct ReqwestFetcher;

impl RemoteScriptFetcher for ReqwestFetcher {
    fn fetch(&self, url: &str) -> Result<String> {
        reqwest::blocking::get(url)
            .with_context(|| format!("downloading {url}"))?
            .error_for_status()
            .with_context(|| format!("downloading {url}"))?
            .text()
            .with_context(|| format!("reading response body from {url}"))
    }
}

// --- Filesystem script writer adapter ---

/// Writes script contents to `install_dir/<name>` and sets the executable bit.
pub struct FsScriptWriter;

impl ScriptWriter for FsScriptWriter {
    fn write(&self, name: &str, contents: &str, install_dir: &Path) -> Result<PathBuf> {
        fs::create_dir_all(install_dir).with_context(|| {
            format!("creating install directory {}", install_dir.display())
        })?;
        let destination = install_dir.join(name);
        fs::write(&destination, contents)
            .with_context(|| format!("writing {}", destination.display()))?;
        make_executable(&destination)?;
        Ok(destination)
    }
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

// --- Walkdir directory scanner adapter ---

/// Recursively lists all files under a directory using `walkdir`.
pub struct WalkdirScanner;

impl DirectoryScanner for WalkdirScanner {
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

// --- Filesystem script reader adapter ---

/// Reads a script's source text from the local filesystem.
pub struct FsScriptReader;

impl ScriptReader for FsScriptReader {
    fn read(&self, path: &Path) -> Result<String> {
        fs::read_to_string(path).with_context(|| format!("reading script {}", path.display()))
    }
}
