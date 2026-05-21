//! Multi-repo manifest, discovery, and filtering for `rx status`,
//! `rx graph`, and `rx fan` subcommands.

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

// =========================================================================
// Manifest types
// =========================================================================

/// Top-level manifest persisted at `~/.config/rx/repos.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub defaults: Defaults,
    /// Backed by `[[repo]]` blocks in TOML.
    #[serde(rename = "repo", default)]
    pub repos: Vec<RepoMeta>,
}

/// Manifest-wide defaults that apply to discovery.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Defaults {
    /// Root directory used by [`ScanRepoSource`] when no explicit root is
    /// supplied. May contain `~` which is expanded relative to `$HOME`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    /// Directory names to skip while scanning. Matched by basename only,
    /// e.g. `target`, `node_modules`, `.venv`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore: Vec<String>,
}

/// One repository entry. After loading from a manifest, `path` has had
/// `~` expanded and any relative segment resolved against the manifest's
/// parent directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoMeta {
    pub name: String,
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

// =========================================================================
// Default paths
// =========================================================================

/// Expand a leading `~` against `$HOME`. Other path forms are returned
/// unchanged.
pub fn expand_tilde(input: &str) -> Result<PathBuf> {
    if let Some(rest) = input.strip_prefix("~/") {
        let home = std::env::var_os("HOME")
            .ok_or_else(|| anyhow!("cannot expand `~`: HOME is not set"))?;
        return Ok(PathBuf::from(home).join(rest));
    }
    if input == "~" {
        let home = std::env::var_os("HOME")
            .ok_or_else(|| anyhow!("cannot expand `~`: HOME is not set"))?;
        return Ok(PathBuf::from(home));
    }
    Ok(PathBuf::from(input))
}

/// Resolve a manifest-relative path: expand `~`, then if still relative,
/// join against `manifest_dir`.
pub fn resolve_manifest_path(input: &Path, manifest_dir: &Path) -> Result<PathBuf> {
    let as_str = input
        .to_str()
        .ok_or_else(|| anyhow!("path is not valid UTF-8: {}", input.display()))?;
    let expanded = expand_tilde(as_str)?;
    if expanded.is_absolute() {
        Ok(expanded)
    } else {
        Ok(manifest_dir.join(expanded))
    }
}

// =========================================================================
// Manifest load / save
// =========================================================================

/// Load a manifest from disk and resolve repo paths relative to the
/// manifest's parent directory.
pub fn load_manifest(path: &Path) -> Result<Manifest> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("reading manifest {}", path.display()))?;
    let mut manifest: Manifest = toml::from_str(&contents)
        .with_context(|| format!("parsing manifest {}", path.display()))?;

    let manifest_dir = path
        .parent()
        .ok_or_else(|| anyhow!("manifest path has no parent: {}", path.display()))?;

    for repo in &mut manifest.repos {
        repo.path = resolve_manifest_path(&repo.path, manifest_dir).with_context(|| {
            format!(
                "resolving path for repo `{}` in {}",
                repo.name,
                path.display()
            )
        })?;
    }

    validate_manifest(&manifest)?;
    Ok(manifest)
}

/// Serialize and write a manifest. Creates parent directories if needed.
pub fn save_manifest(path: &Path, manifest: &Manifest) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating manifest directory {}", parent.display()))?;
    }
    let contents = toml::to_string_pretty(manifest)
        .with_context(|| format!("serializing manifest {}", path.display()))?;
    fs::write(path, contents).with_context(|| format!("writing manifest {}", path.display()))?;
    Ok(())
}

fn validate_manifest(manifest: &Manifest) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    for repo in &manifest.repos {
        if repo.name.trim().is_empty() {
            bail!("manifest contains a repo with an empty name");
        }
        if !seen.insert(repo.name.as_str()) {
            bail!("duplicate repo name in manifest: {}", repo.name);
        }
    }
    Ok(())
}

// =========================================================================
// Discovery
// =========================================================================

/// Walk `root` and return one `RepoMeta` for every directory containing a
/// `.git` entry. Directories whose basename appears in `ignore` are pruned
/// before descent.
///
/// The returned `RepoMeta` has `name = path.file_name()` and no tags or
/// language metadata; callers may enrich it afterwards.
pub fn discover(root: &Path, ignore: &[String]) -> Result<Vec<RepoMeta>> {
    if !root.exists() {
        bail!("scan root does not exist: {}", root.display());
    }

    let mut repos = Vec::new();
    let walker = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !is_ignored(entry, ignore));

    for entry in walker {
        let entry = entry.with_context(|| format!("scanning {}", root.display()))?;
        if !entry.file_type().is_dir() {
            continue;
        }
        let path = entry.path();
        if !is_git_repo(path) {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow!("repo path has no UTF-8 basename: {}", path.display()))?
            .to_string();
        repos.push(RepoMeta {
            name,
            path: path.to_path_buf(),
            role: None,
            language: None,
            default_branch: None,
            tags: Vec::new(),
        });
    }

    repos.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(repos)
}

/// Returns true if `path` is the root of a git repository (contains a
/// `.git` directory or file — the latter for git worktrees).
pub fn is_git_repo(path: &Path) -> bool {
    let dot_git = path.join(".git");
    dot_git.is_dir() || dot_git.is_file()
}

fn is_ignored(entry: &walkdir::DirEntry, ignore: &[String]) -> bool {
    if entry.depth() == 0 {
        return false;
    }
    let Some(name) = entry.file_name().to_str() else {
        return false;
    };
    ignore.iter().any(|i| i == name)
}

// =========================================================================
// Filters
// =========================================================================

/// A single filter expression. Multiple filters are AND-ed by
/// [`apply_filters`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter {
    /// `tag=<value>` — repo's `tags` contains `value`.
    Tag(String),
    /// `role=<value>` — repo's `role` equals `value`.
    Role(String),
    /// `language=<value>` — repo's `language` equals `value`.
    Language(String),
    /// `name=<value>` — repo's `name` equals `value`.
    Name(String),
    /// `name~<glob>` — repo's `name` matches the glob (only `*` supported).
    NameGlob(String),
}

/// Parse a filter expression of the form `key=value` or `name~glob`.
///
/// Recognized keys: `tag`, `role`, `language`, `name`. The `name~` form
/// uses simple glob matching with `*` as the only wildcard.
pub fn parse_filter(input: &str) -> Result<Filter> {
    if let Some((key, value)) = input.split_once('=') {
        let value = value.trim().to_string();
        if value.is_empty() {
            bail!("filter value is empty: {input}");
        }
        return match key.trim() {
            "tag" => Ok(Filter::Tag(value)),
            "role" => Ok(Filter::Role(value)),
            "language" => Ok(Filter::Language(value)),
            "name" => Ok(Filter::Name(value)),
            other => Err(anyhow!("unknown filter key: {other}")),
        };
    }
    if let Some((key, value)) = input.split_once('~') {
        let value = value.trim().to_string();
        if value.is_empty() {
            bail!("filter glob is empty: {input}");
        }
        return match key.trim() {
            "name" => Ok(Filter::NameGlob(value)),
            other => Err(anyhow!("unknown glob filter key: {other}")),
        };
    }
    Err(anyhow!(
        "could not parse filter `{input}` (expected key=value or name~glob)"
    ))
}

/// True iff `repo` satisfies `filter`.
pub fn matches(repo: &RepoMeta, filter: &Filter) -> bool {
    match filter {
        Filter::Tag(t) => repo.tags.iter().any(|x| x == t),
        Filter::Role(r) => repo.role.as_deref() == Some(r.as_str()),
        Filter::Language(l) => repo.language.as_deref() == Some(l.as_str()),
        Filter::Name(n) => repo.name == *n,
        Filter::NameGlob(g) => glob_match(g, &repo.name),
    }
}

/// Apply every filter (AND) and return the matching repos in input order.
pub fn apply_filters(repos: &[RepoMeta], filters: &[Filter]) -> Vec<RepoMeta> {
    repos
        .iter()
        .filter(|r| filters.iter().all(|f| matches(r, f)))
        .cloned()
        .collect()
}

/// Tiny glob matcher supporting only `*` as a wildcard. No escaping.
fn glob_match(pattern: &str, candidate: &str) -> bool {
    // Split on '*' so each segment must appear in order.
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == candidate;
    }

    let mut cursor = 0usize;
    let bytes = candidate.as_bytes();

    // Leading segment must match the start.
    let first = parts[0];
    if !first.is_empty() {
        if !candidate.starts_with(first) {
            return false;
        }
        cursor = first.len();
    }

    // Trailing segment must match the end.
    let last = parts.last().copied().unwrap_or("");
    let middle = &parts[1..parts.len().saturating_sub(1)];

    for seg in middle {
        if seg.is_empty() {
            continue;
        }
        let Some(found) = candidate[cursor..].find(seg) else {
            return false;
        };
        cursor += found + seg.len();
    }

    if !last.is_empty() {
        if cursor > bytes.len() {
            return false;
        }
        if !candidate[cursor..].ends_with(last) {
            return false;
        }
    }
    true
}

// =========================================================================
// Ports + adapters
// =========================================================================

/// Source of repo metadata. Adapters provide manifest-backed or
/// filesystem-backed implementations.
pub trait RepoSource {
    fn list(&self) -> Result<Vec<RepoMeta>>;
}

/// Reads repos straight out of an in-memory [`Manifest`].
pub struct ManifestRepoSource {
    manifest: Manifest,
}

impl ManifestRepoSource {
    pub fn new(manifest: Manifest) -> Self {
        Self { manifest }
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
}

impl RepoSource for ManifestRepoSource {
    fn list(&self) -> Result<Vec<RepoMeta>> {
        Ok(self.manifest.repos.clone())
    }
}

/// Walks a directory and returns every nested git repo.
pub struct ScanRepoSource {
    root: PathBuf,
    ignore: Vec<String>,
}

impl ScanRepoSource {
    pub fn new(root: PathBuf, ignore: Vec<String>) -> Self {
        Self { root, ignore }
    }

    /// Build a scan source from the manifest's `defaults`. The `root` is
    /// required either in the manifest or as an explicit override.
    pub fn from_defaults(defaults: &Defaults, override_root: Option<PathBuf>) -> Result<Self> {
        let root = match override_root {
            Some(p) => p,
            None => match defaults.root.as_deref() {
                Some(r) => expand_tilde(r)?,
                None => bail!("no scan root configured (set [defaults] root in repos.toml)"),
            },
        };
        Ok(Self::new(root, defaults.ignore.clone()))
    }
}

impl RepoSource for ScanRepoSource {
    fn list(&self) -> Result<Vec<RepoMeta>> {
        discover(&self.root, &self.ignore)
    }
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn touch(p: &Path) {
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(p, "").unwrap();
    }

    fn make_git_repo(p: &Path) {
        fs::create_dir_all(p.join(".git")).unwrap();
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

    // --- Manifest --------------------------------------------------------

    #[test]
    fn load_manifest_parses_minimal_toml() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("repos.toml");
        fs::write(
            &path,
            r#"
[defaults]
root = "~/dev"
ignore = ["target", "node_modules"]

[[repo]]
name = "devloop"
path = "/Users/joe/dev/devloop"
role = "lib"
language = "rust"
tags = ["rust", "active"]

[[repo]]
name = "doob"
path = "/Users/joe/dev/doob"
"#,
        )
        .unwrap();

        let manifest = load_manifest(&path).unwrap();
        assert_eq!(manifest.defaults.root.as_deref(), Some("~/dev"));
        assert_eq!(manifest.defaults.ignore, vec!["target", "node_modules"]);
        assert_eq!(manifest.repos.len(), 2);
        assert_eq!(manifest.repos[0].name, "devloop");
        assert_eq!(manifest.repos[0].role.as_deref(), Some("lib"));
        assert_eq!(manifest.repos[0].language.as_deref(), Some("rust"));
        assert_eq!(manifest.repos[0].tags, vec!["rust", "active"]);
        assert_eq!(manifest.repos[1].name, "doob");
        assert!(manifest.repos[1].tags.is_empty());
    }

    #[test]
    fn load_manifest_resolves_relative_paths_against_manifest_dir() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("repos.toml");
        fs::write(
            &path,
            r#"
[[repo]]
name = "near"
path = "subdir/near"
"#,
        )
        .unwrap();

        let manifest = load_manifest(&path).unwrap();
        assert_eq!(manifest.repos[0].path, dir.path().join("subdir/near"));
    }

    #[test]
    fn load_manifest_rejects_duplicate_names() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("repos.toml");
        fs::write(
            &path,
            r#"
[[repo]]
name = "x"
path = "/x"
[[repo]]
name = "x"
path = "/y"
"#,
        )
        .unwrap();
        let err = load_manifest(&path).unwrap_err();
        assert!(err.to_string().contains("duplicate repo name"));
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("repos.toml");
        let original = Manifest {
            defaults: Defaults {
                root: Some("/tmp".into()),
                ignore: vec!["target".into()],
            },
            repos: vec![RepoMeta {
                name: "a".into(),
                path: PathBuf::from("/tmp/a"),
                role: Some("lib".into()),
                language: Some("rust".into()),
                default_branch: Some("main".into()),
                tags: vec!["rust".into()],
            }],
        };
        save_manifest(&path, &original).unwrap();
        let reloaded = load_manifest(&path).unwrap();
        assert_eq!(reloaded, original);
    }

    // --- Discovery -------------------------------------------------------

    #[test]
    fn discover_finds_nested_git_repos_and_skips_ignored_dirs() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        make_git_repo(&root.join("alpha"));
        make_git_repo(&root.join("nested/beta"));
        // Ignored directory, should not be descended into.
        make_git_repo(&root.join("target/should-skip"));
        // A non-repo directory.
        fs::create_dir_all(root.join("not-a-repo")).unwrap();
        // A worktree-style .git file (not directory).
        fs::create_dir_all(root.join("worktree")).unwrap();
        touch(&root.join("worktree/.git"));

        let mut repos = discover(root, &["target".into()]).unwrap();
        repos.sort_by(|a, b| a.name.cmp(&b.name));
        let names: Vec<_> = repos.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "beta", "worktree"]);
    }

    #[test]
    fn discover_errors_when_root_missing() {
        let err = discover(Path::new("/no/such/path/here"), &[]).unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn is_git_repo_true_for_dir_and_file_dot_git() {
        let dir = TempDir::new().unwrap();
        let with_dir = dir.path().join("a");
        let with_file = dir.path().join("b");
        make_git_repo(&with_dir);
        fs::create_dir_all(&with_file).unwrap();
        touch(&with_file.join(".git"));
        assert!(is_git_repo(&with_dir));
        assert!(is_git_repo(&with_file));
        assert!(!is_git_repo(dir.path()));
    }

    // --- Filters ---------------------------------------------------------

    #[test]
    fn parse_filter_handles_known_keys() {
        assert_eq!(
            parse_filter("tag=rust").unwrap(),
            Filter::Tag("rust".into())
        );
        assert_eq!(
            parse_filter("role=lib").unwrap(),
            Filter::Role("lib".into())
        );
        assert_eq!(
            parse_filter("language=rust").unwrap(),
            Filter::Language("rust".into())
        );
        assert_eq!(
            parse_filter("name=devloop").unwrap(),
            Filter::Name("devloop".into())
        );
        assert_eq!(
            parse_filter("name~dev*").unwrap(),
            Filter::NameGlob("dev*".into())
        );
    }

    #[test]
    fn parse_filter_rejects_unknown_keys() {
        assert!(parse_filter("color=red").is_err());
        assert!(parse_filter("name~").is_err());
        assert!(parse_filter("nothing").is_err());
    }

    #[test]
    fn matches_basic_keys() {
        let mut r = meta("devloop");
        r.tags = vec!["rust".into(), "active".into()];
        r.role = Some("lib".into());
        r.language = Some("rust".into());

        assert!(matches(&r, &Filter::Tag("rust".into())));
        assert!(!matches(&r, &Filter::Tag("python".into())));
        assert!(matches(&r, &Filter::Role("lib".into())));
        assert!(matches(&r, &Filter::Language("rust".into())));
        assert!(matches(&r, &Filter::Name("devloop".into())));
        assert!(!matches(&r, &Filter::Name("doob".into())));
    }

    #[test]
    fn glob_matcher_handles_basic_patterns() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("dev*", "devloop"));
        assert!(glob_match("*loop", "devloop"));
        assert!(glob_match("dev*loop", "devXXXloop"));
        assert!(glob_match("a*b*c", "axbyc"));
        assert!(!glob_match("dev*", "doob"));
        assert!(!glob_match("a*b*c", "axby"));
        assert!(glob_match("exact", "exact"));
        assert!(!glob_match("exact", "exactly"));
    }

    #[test]
    fn apply_filters_intersects_all() {
        let mut a = meta("alpha");
        a.tags = vec!["rust".into()];
        a.role = Some("lib".into());
        let mut b = meta("beta");
        b.tags = vec!["rust".into()];
        b.role = Some("app".into());
        let mut c = meta("charlie");
        c.tags = vec!["go".into()];
        c.role = Some("lib".into());

        let repos = vec![a.clone(), b.clone(), c.clone()];
        let filters = vec![Filter::Tag("rust".into()), Filter::Role("lib".into())];
        let out = apply_filters(&repos, &filters);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "alpha");
    }

    // --- Adapters --------------------------------------------------------

    #[test]
    fn manifest_repo_source_returns_manifest_repos() {
        let manifest = Manifest {
            defaults: Defaults::default(),
            repos: vec![meta("a"), meta("b")],
        };
        let src = ManifestRepoSource::new(manifest);
        let repos = src.list().unwrap();
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].name, "a");
    }

    #[test]
    fn scan_repo_source_walks_root() {
        let dir = TempDir::new().unwrap();
        make_git_repo(&dir.path().join("alpha"));
        make_git_repo(&dir.path().join("beta"));

        let src = ScanRepoSource::new(dir.path().to_path_buf(), vec![]);
        let repos = src.list().unwrap();
        let names: Vec<_> = repos.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn scan_from_defaults_requires_root_or_override() {
        let defaults = Defaults::default();
        assert!(ScanRepoSource::from_defaults(&defaults, None).is_err());

        let dir = TempDir::new().unwrap();
        let with_override =
            ScanRepoSource::from_defaults(&Defaults::default(), Some(dir.path().to_path_buf()))
                .unwrap();
        assert_eq!(with_override.list().unwrap(), Vec::<RepoMeta>::new());
    }
}
