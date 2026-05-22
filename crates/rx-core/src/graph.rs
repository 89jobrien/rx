//! Cross-repo Cargo dependency graph.
//!
//! Scans repos for `Cargo.toml` files, extracts package names and
//! dependencies, then builds a directed graph with cross-repo edges.

use crate::repo::RepoMeta;
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};

// =========================================================================
// Domain types
// =========================================================================

#[derive(Debug, Clone, Serialize)]
pub struct PackageNode {
    pub name: String,
    pub version: String,
    pub repo: String,
    pub manifest_path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct DepEdge {
    pub from: String,
    pub to: String,
    pub kind: DepKind,
    pub req: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DepKind {
    Normal,
    Dev,
    Build,
}

#[derive(Debug, Clone, Serialize)]
pub struct DepGraph {
    pub packages: BTreeMap<String, PackageNode>,
    pub edges: Vec<DepEdge>,
}

// =========================================================================
// Port
// =========================================================================

/// Scans a repo directory for Cargo.toml files and extracts package info.
pub trait CargoScanner: Sync {
    fn scan_repo(&self, repo: &RepoMeta) -> Result<Vec<CrateInfo>>;
}

/// Extracted info from a single Cargo.toml.
#[derive(Debug, Clone)]
pub struct CrateInfo {
    pub name: String,
    pub version: String,
    pub repo_name: String,
    pub manifest_path: PathBuf,
    pub deps: Vec<RawDep>,
}

/// A dependency entry extracted from Cargo.toml.
#[derive(Debug, Clone)]
pub struct RawDep {
    pub name: String,
    pub kind: DepKind,
    pub req: String,
}

// =========================================================================
// Adapter: filesystem Cargo.toml scanner
// =========================================================================

pub struct FsCargoScanner;

impl CargoScanner for FsCargoScanner {
    fn scan_repo(&self, repo: &RepoMeta) -> Result<Vec<CrateInfo>> {
        let root_manifest = repo.path.join("Cargo.toml");
        if !root_manifest.exists() {
            return Ok(Vec::new());
        }

        let root_content = std::fs::read_to_string(&root_manifest)
            .with_context(|| format!("reading {}", root_manifest.display()))?;
        let root_toml: toml::Value = toml::from_str(&root_content)
            .with_context(|| format!("parsing {}", root_manifest.display()))?;

        // Check if this is a workspace
        let member_paths = workspace_member_paths(&root_toml, &repo.path);

        let mut crates = Vec::new();

        // Parse root package if it has one
        if root_toml.get("package").is_some()
            && let Some(info) = parse_cargo_toml(&root_content, &root_manifest, &repo.name)
        {
            crates.push(info);
        }

        // Parse workspace members
        for member_dir in member_paths {
            let member_manifest = member_dir.join("Cargo.toml");
            if !member_manifest.exists() {
                continue;
            }
            let content = match std::fs::read_to_string(&member_manifest) {
                Ok(c) => c,
                Err(_) => continue,
            };
            if let Some(info) = parse_cargo_toml(&content, &member_manifest, &repo.name) {
                crates.push(info);
            }
        }

        Ok(crates)
    }
}

fn workspace_member_paths(root_toml: &toml::Value, repo_root: &Path) -> Vec<PathBuf> {
    let Some(workspace) = root_toml.get("workspace") else {
        return Vec::new();
    };
    let Some(members) = workspace.get("members").and_then(|m| m.as_array()) else {
        return Vec::new();
    };

    let mut paths = Vec::new();
    for member in members {
        let Some(pattern) = member.as_str() else {
            continue;
        };
        // Handle simple glob patterns like "crates/*"
        if pattern.contains('*') {
            let prefix = pattern.trim_end_matches("/*").trim_end_matches("/*");
            let parent = repo_root.join(prefix);
            if let Ok(entries) = std::fs::read_dir(&parent) {
                for entry in entries.flatten() {
                    if entry.path().join("Cargo.toml").exists() {
                        paths.push(entry.path());
                    }
                }
            }
        } else {
            paths.push(repo_root.join(pattern));
        }
    }
    paths
}

fn parse_cargo_toml(content: &str, path: &Path, repo_name: &str) -> Option<CrateInfo> {
    let toml: toml::Value = toml::from_str(content).ok()?;
    let pkg = toml.get("package")?;
    let name = pkg.get("name")?.as_str()?.to_string();
    let version = pkg
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("0.0.0")
        .to_string();

    let mut deps = Vec::new();
    extract_deps(&toml, "dependencies", DepKind::Normal, &mut deps);
    extract_deps(&toml, "dev-dependencies", DepKind::Dev, &mut deps);
    extract_deps(&toml, "build-dependencies", DepKind::Build, &mut deps);

    Some(CrateInfo {
        name,
        version,
        repo_name: repo_name.to_string(),
        manifest_path: path.to_path_buf(),
        deps,
    })
}

fn extract_deps(toml: &toml::Value, section: &str, kind: DepKind, out: &mut Vec<RawDep>) {
    let Some(table) = toml.get(section).and_then(|v| v.as_table()) else {
        return;
    };
    for (name, value) in table {
        let req = match value {
            toml::Value::String(s) => s.clone(),
            toml::Value::Table(t) => t
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("*")
                .to_string(),
            _ => "*".to_string(),
        };
        out.push(RawDep {
            name: name.clone(),
            kind,
            req,
        });
    }
}

// =========================================================================
// Graph building
// =========================================================================

/// Build a dependency graph from repos using the given scanner.
pub fn build_graph(repos: &[RepoMeta], scanner: &impl CargoScanner) -> Result<DepGraph> {
    let mut packages = BTreeMap::new();
    let mut all_crates = Vec::new();

    for repo in repos {
        let crates = scanner.scan_repo(repo)?;
        for c in crates {
            packages.insert(
                c.name.clone(),
                PackageNode {
                    name: c.name.clone(),
                    version: c.version.clone(),
                    repo: c.repo_name.clone(),
                    manifest_path: c.manifest_path.clone(),
                },
            );
            all_crates.push(c);
        }
    }

    // Build edges only for deps that reference known packages
    let known: BTreeSet<&str> = packages.keys().map(|s| s.as_str()).collect();
    let mut edges = Vec::new();
    for c in &all_crates {
        for dep in &c.deps {
            if known.contains(dep.name.as_str()) {
                edges.push(DepEdge {
                    from: c.name.clone(),
                    to: dep.name.clone(),
                    kind: dep.kind,
                    req: dep.req.clone(),
                });
            }
        }
    }

    Ok(DepGraph { packages, edges })
}

// =========================================================================
// Queries
// =========================================================================

impl DepGraph {
    /// Packages that directly or transitively depend on `pkg`.
    pub fn who_uses(&self, pkg: &str) -> Vec<String> {
        // Build reverse adjacency
        let mut rev: HashMap<&str, Vec<&str>> = HashMap::new();
        for edge in &self.edges {
            rev.entry(edge.to.as_str())
                .or_default()
                .push(edge.from.as_str());
        }
        bfs(pkg, &rev)
    }

    /// Transitive dependencies of `pkg`.
    pub fn deps(&self, pkg: &str) -> Vec<String> {
        let mut fwd: HashMap<&str, Vec<&str>> = HashMap::new();
        for edge in &self.edges {
            fwd.entry(edge.from.as_str())
                .or_default()
                .push(edge.to.as_str());
        }
        bfs(pkg, &fwd)
    }

    /// Topological order of all packages (dependencies before dependents).
    pub fn topo_order(&self) -> Vec<String> {
        // Edges are from -> to meaning "from depends on to".
        // For build order we want dependencies first, so reverse edges:
        // in-degree counts how many packages depend on you (reverse direction).
        let mut in_degree: BTreeMap<&str, usize> = BTreeMap::new();
        let mut rev: BTreeMap<&str, Vec<&str>> = BTreeMap::new();

        for name in self.packages.keys() {
            in_degree.entry(name.as_str()).or_insert(0);
        }
        for edge in &self.edges {
            // Reverse: to -> from (dependency is built first)
            rev.entry(edge.to.as_str())
                .or_default()
                .push(edge.from.as_str());
            *in_degree.entry(edge.from.as_str()).or_insert(0) += 1;
        }

        let mut queue: VecDeque<&str> = in_degree
            .iter()
            .filter(|(_, deg)| **deg == 0)
            .map(|(&name, _)| name)
            .collect();

        let mut order = Vec::new();
        while let Some(node) = queue.pop_front() {
            order.push(node.to_string());
            if let Some(neighbors) = rev.get(node) {
                for &neighbor in neighbors {
                    let deg = in_degree.get_mut(neighbor).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(neighbor);
                    }
                }
            }
        }
        order
    }
}

fn bfs<'a>(start: &str, adj: &HashMap<&'a str, Vec<&'a str>>) -> Vec<String> {
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::new();
    if let Some(neighbors) = adj.get(start) {
        for &n in neighbors {
            queue.push_back(n);
        }
    }
    while let Some(node) = queue.pop_front() {
        if !visited.insert(node) {
            continue;
        }
        if let Some(neighbors) = adj.get(node) {
            for &n in neighbors {
                if !visited.contains(n) {
                    queue.push_back(n);
                }
            }
        }
    }
    visited.into_iter().map(String::from).collect()
}

// =========================================================================
// Rendering
// =========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphFormat {
    Tree,
    Json,
    Mermaid,
}

pub fn render_graph(graph: &DepGraph, format: GraphFormat) -> String {
    match format {
        GraphFormat::Json => serde_json::to_string_pretty(graph).unwrap_or_default(),
        GraphFormat::Mermaid => render_mermaid(graph),
        GraphFormat::Tree => render_tree(graph),
    }
}

fn render_mermaid(graph: &DepGraph) -> String {
    let mut out = String::from("graph LR\n");
    for edge in &graph.edges {
        if edge.kind == DepKind::Dev {
            out.push_str(&format!("  {} -.-> {}\n", edge.from, edge.to));
        } else {
            out.push_str(&format!("  {} --> {}\n", edge.from, edge.to));
        }
    }
    out
}

fn render_tree(graph: &DepGraph) -> String {
    // Group packages by repo, show deps under each
    let mut by_repo: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (name, node) in &graph.packages {
        by_repo
            .entry(node.repo.as_str())
            .or_default()
            .push(name.as_str());
    }

    let edge_lookup: HashMap<&str, Vec<&DepEdge>> = {
        let mut m: HashMap<&str, Vec<&DepEdge>> = HashMap::new();
        for edge in &graph.edges {
            m.entry(edge.from.as_str()).or_default().push(edge);
        }
        m
    };

    let mut out = String::new();
    for (repo, pkgs) in &by_repo {
        out.push_str(&format!("{repo}\n"));
        for (i, pkg) in pkgs.iter().enumerate() {
            let is_last_pkg = i == pkgs.len() - 1;
            let prefix = if is_last_pkg {
                "  └── "
            } else {
                "  ├── "
            };
            let node = &graph.packages[*pkg];
            out.push_str(&format!("{prefix}{} v{}\n", pkg, node.version));

            if let Some(edges) = edge_lookup.get(pkg) {
                let internal: Vec<&&DepEdge> = edges
                    .iter()
                    .filter(|e| graph.packages.contains_key(&e.to))
                    .collect();
                for (j, edge) in internal.iter().enumerate() {
                    let child_prefix = if is_last_pkg { "      " } else { "  │   " };
                    let connector = if j == internal.len() - 1 {
                        "└── "
                    } else {
                        "├── "
                    };
                    let target_repo = &graph.packages[&edge.to].repo;
                    let kind_label = match edge.kind {
                        DepKind::Normal => "",
                        DepKind::Dev => " (dev)",
                        DepKind::Build => " (build)",
                    };
                    out.push_str(&format!(
                        "{child_prefix}{connector}{}{kind_label} [{target_repo}]\n",
                        edge.to
                    ));
                }
            }
        }
    }
    out
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_crate(name: &str, repo: &str, deps: &[(&str, DepKind)]) -> CrateInfo {
        CrateInfo {
            name: name.to_string(),
            version: "0.1.0".to_string(),
            repo_name: repo.to_string(),
            manifest_path: PathBuf::from(format!("/tmp/{repo}/{name}/Cargo.toml")),
            deps: deps
                .iter()
                .map(|(n, k)| RawDep {
                    name: n.to_string(),
                    kind: *k,
                    req: "0.1".to_string(),
                })
                .collect(),
        }
    }

    struct FakeScanner {
        crates: Vec<CrateInfo>,
    }

    impl CargoScanner for FakeScanner {
        fn scan_repo(&self, repo: &RepoMeta) -> Result<Vec<CrateInfo>> {
            Ok(self
                .crates
                .iter()
                .filter(|c| c.repo_name == repo.name)
                .cloned()
                .collect())
        }
    }

    fn meta(name: &str) -> RepoMeta {
        RepoMeta {
            name: name.to_string(),
            path: PathBuf::from(format!("/tmp/{name}")),
            role: None,
            language: None,
            default_branch: None,
            tags: Vec::new(),
        }
    }

    #[test]
    fn build_graph_finds_cross_repo_edges() {
        let scanner = FakeScanner {
            crates: vec![
                make_crate("lib-a", "repo-a", &[]),
                make_crate("app-b", "repo-b", &[("lib-a", DepKind::Normal)]),
                make_crate(
                    "app-c",
                    "repo-c",
                    &[
                        ("lib-a", DepKind::Normal),
                        ("app-b", DepKind::Dev),
                        ("serde", DepKind::Normal), // external, should not appear
                    ],
                ),
            ],
        };
        let repos = vec![meta("repo-a"), meta("repo-b"), meta("repo-c")];
        let graph = build_graph(&repos, &scanner).unwrap();

        assert_eq!(graph.packages.len(), 3);
        assert_eq!(graph.edges.len(), 3); // lib-a<-app-b, lib-a<-app-c, app-b<-app-c(dev)
        assert!(graph.edges.iter().all(|e| e.to != "serde"));
    }

    #[test]
    fn who_uses_returns_transitive_dependents() {
        let scanner = FakeScanner {
            crates: vec![
                make_crate("core", "r", &[]),
                make_crate("mid", "r", &[("core", DepKind::Normal)]),
                make_crate("top", "r", &[("mid", DepKind::Normal)]),
            ],
        };
        let graph = build_graph(&[meta("r")], &scanner).unwrap();
        let users = graph.who_uses("core");
        assert!(users.contains(&"mid".to_string()));
        assert!(users.contains(&"top".to_string()));
    }

    #[test]
    fn deps_returns_transitive_dependencies() {
        let scanner = FakeScanner {
            crates: vec![
                make_crate("core", "r", &[]),
                make_crate("mid", "r", &[("core", DepKind::Normal)]),
                make_crate("top", "r", &[("mid", DepKind::Normal)]),
            ],
        };
        let graph = build_graph(&[meta("r")], &scanner).unwrap();
        let d = graph.deps("top");
        assert!(d.contains(&"mid".to_string()));
        assert!(d.contains(&"core".to_string()));
    }

    #[test]
    fn topo_order_is_valid() {
        let scanner = FakeScanner {
            crates: vec![
                make_crate("a", "r", &[]),
                make_crate("b", "r", &[("a", DepKind::Normal)]),
                make_crate("c", "r", &[("b", DepKind::Normal)]),
            ],
        };
        let graph = build_graph(&[meta("r")], &scanner).unwrap();
        let order = graph.topo_order();
        let pos = |name: &str| order.iter().position(|n| n == name).unwrap();
        assert!(pos("a") < pos("b"));
        assert!(pos("b") < pos("c"));
    }

    #[test]
    fn render_mermaid_output() {
        let scanner = FakeScanner {
            crates: vec![
                make_crate("a", "r", &[]),
                make_crate("b", "r", &[("a", DepKind::Normal)]),
            ],
        };
        let graph = build_graph(&[meta("r")], &scanner).unwrap();
        let mermaid = render_graph(&graph, GraphFormat::Mermaid);
        assert!(mermaid.contains("graph LR"));
        assert!(mermaid.contains("b --> a"));
    }

    #[test]
    fn render_json_is_valid() {
        let scanner = FakeScanner {
            crates: vec![make_crate("a", "r", &[])],
        };
        let graph = build_graph(&[meta("r")], &scanner).unwrap();
        let json = render_graph(&graph, GraphFormat::Json);
        let _: serde_json::Value = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn parse_cargo_toml_extracts_deps() {
        let toml = r#"
[package]
name = "my-crate"
version = "1.2.3"

[dependencies]
serde = "1.0"
tokio = { version = "1", features = ["full"] }

[dev-dependencies]
tempfile = "3"
"#;
        let info = parse_cargo_toml(toml, Path::new("/tmp/Cargo.toml"), "test").unwrap();
        assert_eq!(info.name, "my-crate");
        assert_eq!(info.version, "1.2.3");
        assert_eq!(info.deps.len(), 3);

        let serde_dep = info.deps.iter().find(|d| d.name == "serde").unwrap();
        assert_eq!(serde_dep.kind, DepKind::Normal);
        assert_eq!(serde_dep.req, "1.0");

        let temp_dep = info.deps.iter().find(|d| d.name == "tempfile").unwrap();
        assert_eq!(temp_dep.kind, DepKind::Dev);
    }

    #[test]
    fn empty_graph_queries_are_safe() {
        let graph = DepGraph {
            packages: BTreeMap::new(),
            edges: Vec::new(),
        };
        assert!(graph.who_uses("nonexistent").is_empty());
        assert!(graph.deps("nonexistent").is_empty());
        assert!(graph.topo_order().is_empty());
    }
}
