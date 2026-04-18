use anyhow::{Context, Result, anyhow};
use rx_script_core::{InstalledScript, RegistryEntry, RegistryStore, RemoteScriptFetcher};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

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

pub struct JsonRegistryStore {
    path: PathBuf,
}

impl JsonRegistryStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

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
