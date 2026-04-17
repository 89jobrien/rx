use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use rx_core::{
    CommandPrefixConfig, ExecutionPlan, InstallRequest, RunRequest, apply_command_prefix,
    format_registry_entry, install, list_installed, plan_installed_run,
};
use rx_registry_json::{JsonRegistryStore, ReqwestFetcher, default_paths};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio, exit},
};

const OP_PLUGIN_PREFIX: &[&str] = &["op", "plugin", "run", "--"];
const DOTENVX_PREFIX: &[&str] = &["dotenvx", "run", "--"];
const AI_DOTENVX_COMMANDS: &[&str] = &[
    "gemini",
    "claude",
    "codex",
    "ollama",
    "opencode",
    "pi",
    "deepagents",
    "toolz",
];
type AliasExpansion = (String, Vec<String>);
type AliasParser = fn(&str) -> Option<AliasExpansion>;

#[derive(Debug, Parser)]
#[command(
    name = "rx",
    about = "Install compatible scripts from local or remote sources"
)]
struct Cli {
    #[arg(
        long,
        global = true,
        value_name = "FILE",
        default_value_os_t = default_prefix_config_path()
    )]
    prefix_config: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Install {
        source: String,
        #[arg(long, value_name = "DIR", default_value_os_t = default_install_dir())]
        install_dir: PathBuf,
    },
    List {
        #[arg(long, value_name = "FILE", default_value_os_t = default_registry_path())]
        registry_path: PathBuf,
    },
    #[command(trailing_var_arg = true)]
    Run {
        name: String,
        #[arg(long, value_name = "FILE", default_value_os_t = default_registry_path())]
        registry_path: PathBuf,
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(external_subcommand)]
    External(Vec<String>),
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let shell_aliases = discover_shell_command_expansions()?;

    match cli.command {
        Command::Install {
            source,
            install_dir,
        } => {
            let paths = default_paths()?;
            let mut registry = JsonRegistryStore::new(paths.registry_path);
            let report = install(
                &InstallRequest {
                    source,
                    install_dir,
                },
                &mut registry,
                &ReqwestFetcher,
            )?;

            for script in &report.installed {
                println!(
                    "installed {} ({}) -> {}",
                    script.name,
                    script.source,
                    script.destination.display()
                );
            }

            if !report.skipped.is_empty() {
                eprintln!("skipped incompatible files:");
                for item in &report.skipped {
                    eprintln!("  {item}");
                }
            }
        }
        Command::List { registry_path } => {
            let registry = JsonRegistryStore::new(registry_path);
            for entry in list_installed(&registry)? {
                println!("{}", format_registry_entry(&entry));
            }
        }
        Command::Run {
            name,
            registry_path,
            args,
        } => {
            let registry = JsonRegistryStore::new(registry_path);
            let plan = plan_installed_run(&RunRequest { name, args }, &registry)?;
            let status = execute_plan(&ProcessRunner, &plan, &cli.prefix_config)?;
            exit_with_status(status);
        }
        Command::External(args) => {
            let plan = plan_external_command(&args, &shell_aliases)?;
            let status = execute_plan(&ProcessRunner, &plan, &cli.prefix_config)?;
            exit_with_status(status);
        }
    }

    Ok(())
}

fn default_install_dir() -> PathBuf {
    default_paths()
        .map(|paths| paths.bin_dir)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn default_registry_path() -> PathBuf {
    default_paths()
        .map(|paths| paths.registry_path)
        .unwrap_or_else(|_| PathBuf::from("registry.json"))
}

fn default_prefix_config_path() -> PathBuf {
    default_paths()
        .map(|paths| paths.root.join("prefixes.toml"))
        .unwrap_or_else(|_| PathBuf::from("prefixes.toml"))
}

fn plan_external_command(
    args: &[String],
    aliases: &std::collections::BTreeMap<String, Vec<String>>,
) -> Result<ExecutionPlan> {
    let (program, rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("no external command was provided"))?;

    if let Some(expansion) = aliases.get(program) {
        let (expanded_program, expanded_args) = expansion
            .split_first()
            .ok_or_else(|| anyhow!("alias expansion cannot be empty"))?;
        let mut args = expanded_args.to_vec();
        args.extend(rest.to_vec());
        return Ok(ExecutionPlan {
            program: expanded_program.clone(),
            args,
        });
    }

    Ok(ExecutionPlan {
        program: program.clone(),
        args: rest.to_vec(),
    })
}

trait PlanRunner {
    fn run(&self, plan: &ExecutionPlan) -> Result<ExitStatus>;
}

struct ProcessRunner;

impl PlanRunner for ProcessRunner {
    fn run(&self, plan: &ExecutionPlan) -> Result<ExitStatus> {
        std::process::Command::new(&plan.program)
            .args(&plan.args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .with_context(|| format!("running {}", plan.program))
    }
}

fn execute_plan<R: PlanRunner>(
    runner: &R,
    plan: &ExecutionPlan,
    prefix_config_path: &Path,
) -> Result<ExitStatus> {
    let mut prefix_config = load_prefix_config(prefix_config_path)?;

    if let Some(prefix) = prefix_config.mappings.get(&plan.program) {
        let prefixed = apply_command_prefix(plan, prefix)?;
        return runner.run(&prefixed);
    }

    let base_result = runner.run(plan);
    if !should_try_candidate_prefixes(&base_result, &prefix_config) {
        return base_result;
    }

    for candidate in &prefix_config.candidate_prefixes {
        let prefixed = apply_command_prefix(plan, candidate)?;
        let candidate_result = runner.run(&prefixed);
        if matches!(candidate_result, Ok(status) if status.success()) {
            prefix_config
                .mappings
                .insert(plan.program.clone(), candidate.clone());
            save_prefix_config(prefix_config_path, &prefix_config)?;
            return candidate_result;
        }
    }

    base_result
}

fn should_try_candidate_prefixes(
    base_result: &Result<ExitStatus>,
    prefix_config: &CommandPrefixConfig,
) -> bool {
    prefix_config.learn_on_successful_fallback
        && !prefix_config.candidate_prefixes.is_empty()
        && match base_result {
            Ok(status) => !status.success(),
            Err(_) => true,
        }
}

fn load_prefix_config(path: &Path) -> Result<CommandPrefixConfig> {
    load_prefix_config_with_defaults(path, discover_prefix_config()?)
}

fn load_prefix_config_with_defaults(
    path: &Path,
    discovered: CommandPrefixConfig,
) -> Result<CommandPrefixConfig> {
    if !path.exists() {
        return Ok(discovered);
    }

    let contents = fs::read_to_string(path)
        .with_context(|| format!("reading prefix config {}", path.display()))?;
    let configured: CommandPrefixConfig = toml::from_str(&contents)
        .with_context(|| format!("parsing prefix config {}", path.display()))?;
    Ok(merge_prefix_configs(discovered, configured))
}

fn save_prefix_config(path: &Path, config: &CommandPrefixConfig) -> Result<()> {
    if config.candidate_prefixes.iter().any(Vec::is_empty) {
        bail!("candidate prefixes cannot be empty");
    }
    if config.mappings.values().any(Vec::is_empty) {
        bail!("stored command prefixes cannot be empty");
    }

    let parent = path.parent().ok_or_else(|| {
        anyhow!(
            "prefix config path has no parent directory: {}",
            path.display()
        )
    })?;
    fs::create_dir_all(parent)
        .with_context(|| format!("creating prefix config directory {}", parent.display()))?;

    let contents = toml::to_string_pretty(config)
        .with_context(|| format!("serializing prefix config {}", path.display()))?;
    fs::write(path, contents).with_context(|| format!("writing prefix config {}", path.display()))
}

fn discover_prefix_config() -> Result<CommandPrefixConfig> {
    let mut config = CommandPrefixConfig {
        learn_on_successful_fallback: true,
        ..CommandPrefixConfig::default()
    };

    if let Some(op_plugins_dir) = op_plugins_dir()
        && op_plugins_dir.is_dir()
    {
        push_candidate_prefix(&mut config, string_vec(OP_PLUGIN_PREFIX));

        for plugin_name in configured_op_plugin_names(&op_plugins_dir)? {
            config
                .mappings
                .entry(plugin_name)
                .or_insert_with(|| string_vec(OP_PLUGIN_PREFIX));
        }
    }

    if command_exists("dotenvx") {
        push_candidate_prefix(&mut config, string_vec(DOTENVX_PREFIX));

        for command in discover_dotenvx_commands()? {
            config
                .mappings
                .entry(command)
                .or_insert_with(|| string_vec(DOTENVX_PREFIX));
        }
    }

    Ok(config)
}

fn discover_shell_command_expansions() -> Result<std::collections::BTreeMap<String, Vec<String>>> {
    let mut aliases = std::collections::BTreeMap::new();

    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        merge_alias_file(&mut aliases, &home.join(".zshrc"), parse_zsh_alias_line)?;
        merge_alias_file(
            &mut aliases,
            &home.join(".config").join("fish").join("config.fish"),
            parse_fish_alias_or_abbr_line,
        )?;
    }

    Ok(aliases)
}

fn merge_alias_file(
    aliases: &mut std::collections::BTreeMap<String, Vec<String>>,
    path: &Path,
    parser: AliasParser,
) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let contents = fs::read_to_string(path)
        .with_context(|| format!("reading shell config {}", path.display()))?;
    for line in contents.lines() {
        if let Some((name, expansion)) = parser(line) {
            aliases.insert(name, expansion);
        }
    }

    Ok(())
}

fn parse_zsh_alias_line(line: &str) -> Option<AliasExpansion> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix("alias ")?;
    let (name, value) = rest.split_once('=')?;
    parse_alias_expansion(name.trim(), value.trim())
}

fn parse_fish_alias_or_abbr_line(line: &str) -> Option<AliasExpansion> {
    let trimmed = line.trim();

    if let Some(rest) = trimmed.strip_prefix("alias ") {
        let mut parts = shell_words::split(rest).ok()?;
        if parts.len() < 2 {
            return None;
        }
        let name = parts.remove(0);
        let expansion = parts.join(" ");
        return parse_alias_expansion(&name, &expansion);
    }

    if let Some(rest) = trimmed
        .strip_prefix("abbr -a ")
        .or_else(|| trimmed.strip_prefix("abbr --add "))
    {
        let parts = shell_words::split(rest).ok()?;
        let (name, expansion) = split_fish_abbr_parts(parts)?;
        return validate_alias_expansion(&name, expansion);
    }

    None
}

fn split_fish_abbr_parts(parts: Vec<String>) -> Option<AliasExpansion> {
    let mut iter = parts.into_iter();
    let name = iter.next()?;
    let expansion: Vec<String> = iter.collect();
    if expansion.is_empty() {
        return None;
    }
    Some((name, expansion))
}

fn parse_alias_expansion(name: &str, raw_value: &str) -> Option<AliasExpansion> {
    let stripped = raw_value
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
        .or_else(|| {
            raw_value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
        })
        .unwrap_or(raw_value);
    let parts = shell_words::split(stripped).ok()?;
    validate_alias_expansion(name, parts)
}

fn validate_alias_expansion(name: &str, parts: Vec<String>) -> Option<AliasExpansion> {
    if parts.is_empty() || !is_safe_command_expansion(&parts) {
        return None;
    }
    Some((name.to_string(), parts))
}

fn is_safe_command_expansion(parts: &[String]) -> bool {
    let shell_builtins = [
        "alias", "builtin", "cd", "eval", "exec", "export", "function", "set", "source", ".",
        "unalias",
    ];

    !parts.iter().any(|part| contains_shell_control(part))
        && parts
            .first()
            .is_some_and(|program| !shell_builtins.contains(&program.as_str()))
}

fn contains_shell_control(token: &str) -> bool {
    ["&&", "||", ";", "|", "$(", "`", ">", "<"]
        .iter()
        .any(|needle| token.contains(needle))
}

fn merge_prefix_configs(
    mut discovered: CommandPrefixConfig,
    configured: CommandPrefixConfig,
) -> CommandPrefixConfig {
    for prefix in configured.candidate_prefixes {
        push_candidate_prefix(&mut discovered, prefix);
    }

    for (command, prefix) in configured.mappings {
        discovered.mappings.insert(command, prefix);
    }

    discovered.learn_on_successful_fallback |= configured.learn_on_successful_fallback;
    discovered
}

fn push_candidate_prefix(config: &mut CommandPrefixConfig, prefix: Vec<String>) {
    if !config.candidate_prefixes.contains(&prefix) {
        config.candidate_prefixes.push(prefix);
    }
}

fn configured_op_plugin_names(op_plugins_dir: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();

    for entry in fs::read_dir(op_plugins_dir)
        .with_context(|| format!("reading op plugin directory {}", op_plugins_dir.display()))?
    {
        let entry =
            entry.with_context(|| format!("reading entry in {}", op_plugins_dir.display()))?;
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }

        if let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) {
            names.push(name.to_string());
        }
    }

    names.sort();
    Ok(names)
}

fn discover_dotenvx_commands() -> Result<Vec<String>> {
    let mut commands = BTreeSet::new();

    for command in AI_DOTENVX_COMMANDS {
        if command_exists(command) {
            commands.insert((*command).to_string());
        }
    }

    for command in discover_mise_ai_commands()? {
        if command_exists(&command) {
            commands.insert(command);
        }
    }

    Ok(commands.into_iter().collect())
}

fn discover_mise_ai_commands() -> Result<Vec<String>> {
    let Some(config_home) = config_home_dir() else {
        return Ok(Vec::new());
    };
    let path = config_home.join("mise").join("config.toml");
    if !path.exists() {
        return Ok(Vec::new());
    }

    let contents = fs::read_to_string(&path)
        .with_context(|| format!("reading mise config {}", path.display()))?;
    let value: toml::Value = toml::from_str(&contents)
        .with_context(|| format!("parsing mise config {}", path.display()))?;

    let mut commands = BTreeSet::new();
    let Some(tools) = value.get("tools").and_then(toml::Value::as_table) else {
        return Ok(Vec::new());
    };

    for key in tools.keys() {
        if let Some(command) = command_name_from_mise_tool_key(key) {
            commands.insert(command);
        }
    }

    Ok(commands.into_iter().collect())
}

fn command_name_from_mise_tool_key(key: &str) -> Option<String> {
    let package = key.strip_prefix("npm:")?;
    let package_name = package.rsplit('/').next().unwrap_or(package);

    match package_name {
        "gemini-cli" => Some("gemini".to_string()),
        "opencode-ai" => Some("opencode".to_string()),
        "claude-code-templates" => Some("claude".to_string()),
        "openai" => Some("openai".to_string()),
        "deepagents" => Some("deepagents".to_string()),
        "pi" => Some("pi".to_string()),
        other if other.ends_with("-cli") => Some(other.trim_end_matches("-cli").to_string()),
        _ => None,
    }
}

fn op_plugins_dir() -> Option<PathBuf> {
    config_home_dir().map(|path| path.join("op").join("plugins"))
}

fn config_home_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
}

fn command_exists(command: &str) -> bool {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {command} >/dev/null 2>&1"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn string_vec(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_string()).collect()
}

fn exit_with_status(status: ExitStatus) -> ! {
    exit(status.code().unwrap_or(1));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::RefCell,
        collections::{BTreeMap, VecDeque},
    };
    use tempfile::tempdir;

    struct FakeRunner {
        results: RefCell<VecDeque<Result<ExitStatus, String>>>,
        seen: RefCell<Vec<ExecutionPlan>>,
    }

    impl FakeRunner {
        fn new(results: impl IntoIterator<Item = Result<ExitStatus, String>>) -> Self {
            Self {
                results: RefCell::new(results.into_iter().collect()),
                seen: RefCell::new(Vec::new()),
            }
        }
    }

    impl PlanRunner for FakeRunner {
        fn run(&self, plan: &ExecutionPlan) -> Result<ExitStatus> {
            self.seen.borrow_mut().push(plan.clone());
            match self.results.borrow_mut().pop_front() {
                Some(Ok(status)) => Ok(status),
                Some(Err(message)) => Err(anyhow!(message)),
                None => Err(anyhow!("missing fake runner result")),
            }
        }
    }

    #[cfg(unix)]
    fn exit_status(code: i32) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;

        ExitStatus::from_raw(code << 8)
    }

    fn execute_plan_for_test(
        runner: &FakeRunner,
        plan: &ExecutionPlan,
        prefix_config_path: &Path,
    ) -> Result<ExitStatus> {
        let mut prefix_config =
            load_prefix_config_with_defaults(prefix_config_path, CommandPrefixConfig::default())?;

        if let Some(prefix) = prefix_config.mappings.get(&plan.program) {
            let prefixed = apply_command_prefix(plan, prefix)?;
            return runner.run(&prefixed);
        }

        let base_result = runner.run(plan);
        if !should_try_candidate_prefixes(&base_result, &prefix_config) {
            return base_result;
        }

        for candidate in &prefix_config.candidate_prefixes {
            let prefixed = apply_command_prefix(plan, candidate)?;
            let candidate_result = runner.run(&prefixed);
            if matches!(candidate_result, Ok(status) if status.success()) {
                prefix_config
                    .mappings
                    .insert(plan.program.clone(), candidate.clone());
                save_prefix_config(prefix_config_path, &prefix_config)?;
                return candidate_result;
            }
        }

        base_result
    }

    #[test]
    fn external_command_plan_uses_first_arg_as_program() -> Result<()> {
        let plan = plan_external_command(
            &["gh".to_string(), "issue".to_string(), "list".to_string()],
            &BTreeMap::new(),
        )?;

        assert_eq!(plan.program, "gh");
        assert_eq!(plan.args, vec!["issue".to_string(), "list".to_string()]);
        Ok(())
    }

    #[test]
    fn external_command_plan_expands_simple_aliases() -> Result<()> {
        let aliases = BTreeMap::from([(
            "ghrun".to_string(),
            vec!["gh".to_string(), "run".to_string(), "list".to_string()],
        )]);
        let plan = plan_external_command(
            &["ghrun".to_string(), "--limit".to_string(), "5".to_string()],
            &aliases,
        )?;

        assert_eq!(plan.program, "gh");
        assert_eq!(
            plan.args,
            vec![
                "run".to_string(),
                "list".to_string(),
                "--limit".to_string(),
                "5".to_string()
            ]
        );
        Ok(())
    }

    #[test]
    fn execute_plan_uses_learned_mapping_without_retry() -> Result<()> {
        let temp = tempdir()?;
        let prefix_config_path = temp.path().join("prefixes.toml");
        save_prefix_config(
            &prefix_config_path,
            &CommandPrefixConfig {
                mappings: BTreeMap::from([(
                    "gh".to_string(),
                    vec![
                        "op".to_string(),
                        "plugin".to_string(),
                        "run".to_string(),
                        "--".to_string(),
                    ],
                )]),
                candidate_prefixes: Vec::new(),
                learn_on_successful_fallback: false,
            },
        )?;

        let runner = FakeRunner::new([Ok(exit_status(0))]);
        let status = execute_plan_for_test(
            &runner,
            &ExecutionPlan {
                program: "gh".to_string(),
                args: vec!["auth".to_string(), "status".to_string()],
            },
            &prefix_config_path,
        )?;

        assert!(status.success());
        assert_eq!(runner.seen.borrow().len(), 1);
        assert_eq!(runner.seen.borrow()[0].program, "op");
        assert_eq!(
            runner.seen.borrow()[0].args,
            vec![
                "plugin".to_string(),
                "run".to_string(),
                "--".to_string(),
                "gh".to_string(),
                "auth".to_string(),
                "status".to_string(),
            ]
        );
        Ok(())
    }

    #[test]
    fn execute_plan_learns_successful_candidate_prefix() -> Result<()> {
        let temp = tempdir()?;
        let prefix_config_path = temp.path().join("prefixes.toml");
        save_prefix_config(
            &prefix_config_path,
            &CommandPrefixConfig {
                mappings: BTreeMap::new(),
                candidate_prefixes: vec![vec![
                    "op".to_string(),
                    "plugin".to_string(),
                    "run".to_string(),
                    "--".to_string(),
                ]],
                learn_on_successful_fallback: true,
            },
        )?;

        let runner = FakeRunner::new([Ok(exit_status(1)), Ok(exit_status(0))]);
        let status = execute_plan_for_test(
            &runner,
            &ExecutionPlan {
                program: "gh".to_string(),
                args: vec!["issue".to_string(), "list".to_string()],
            },
            &prefix_config_path,
        )?;

        assert!(status.success());
        assert_eq!(runner.seen.borrow().len(), 2);

        let config =
            load_prefix_config_with_defaults(&prefix_config_path, CommandPrefixConfig::default())?;
        assert_eq!(
            config.mappings.get("gh"),
            Some(&vec![
                "op".to_string(),
                "plugin".to_string(),
                "run".to_string(),
                "--".to_string(),
            ])
        );
        Ok(())
    }

    #[test]
    fn execute_plan_skips_learning_when_base_command_succeeds() -> Result<()> {
        let temp = tempdir()?;
        let prefix_config_path = temp.path().join("prefixes.toml");
        save_prefix_config(
            &prefix_config_path,
            &CommandPrefixConfig {
                mappings: BTreeMap::new(),
                candidate_prefixes: vec![vec![
                    "op".to_string(),
                    "plugin".to_string(),
                    "run".to_string(),
                    "--".to_string(),
                ]],
                learn_on_successful_fallback: true,
            },
        )?;

        let runner = FakeRunner::new([Ok(exit_status(0))]);
        let status = execute_plan_for_test(
            &runner,
            &ExecutionPlan {
                program: "gh".to_string(),
                args: vec!["repo".to_string(), "view".to_string()],
            },
            &prefix_config_path,
        )?;

        assert!(status.success());
        assert_eq!(runner.seen.borrow().len(), 1);

        let config =
            load_prefix_config_with_defaults(&prefix_config_path, CommandPrefixConfig::default())?;
        assert!(config.mappings.is_empty());
        Ok(())
    }

    #[test]
    fn merge_prefix_configs_preserves_discovered_defaults_and_overrides_mappings() {
        let discovered = CommandPrefixConfig {
            mappings: BTreeMap::from([
                ("gh".to_string(), string_vec(OP_PLUGIN_PREFIX)),
                ("gemini".to_string(), string_vec(DOTENVX_PREFIX)),
            ]),
            candidate_prefixes: vec![string_vec(OP_PLUGIN_PREFIX)],
            learn_on_successful_fallback: true,
        };
        let configured = CommandPrefixConfig {
            mappings: BTreeMap::from([("gemini".to_string(), string_vec(OP_PLUGIN_PREFIX))]),
            candidate_prefixes: vec![string_vec(DOTENVX_PREFIX)],
            learn_on_successful_fallback: false,
        };

        let merged = merge_prefix_configs(discovered, configured);

        assert_eq!(
            merged.mappings.get("gh"),
            Some(&string_vec(OP_PLUGIN_PREFIX))
        );
        assert_eq!(
            merged.mappings.get("gemini"),
            Some(&string_vec(OP_PLUGIN_PREFIX))
        );
        assert_eq!(
            merged.candidate_prefixes,
            vec![string_vec(OP_PLUGIN_PREFIX), string_vec(DOTENVX_PREFIX)]
        );
        assert!(merged.learn_on_successful_fallback);
    }

    #[test]
    fn configured_op_plugin_names_reads_json_files_only() -> Result<()> {
        let temp = tempdir()?;
        fs::write(temp.path().join("gh.json"), "{}")?;
        fs::write(temp.path().join("openai.json"), "{}")?;
        fs::create_dir(temp.path().join("used_items"))?;
        fs::write(temp.path().join("notes.txt"), "ignored")?;

        let names = configured_op_plugin_names(temp.path())?;

        assert_eq!(names, vec!["gh".to_string(), "openai".to_string()]);
        Ok(())
    }

    #[test]
    fn command_name_from_mise_tool_key_maps_known_ai_tools() {
        assert_eq!(
            command_name_from_mise_tool_key("npm:@google/gemini-cli"),
            Some("gemini".to_string())
        );
        assert_eq!(
            command_name_from_mise_tool_key("npm:opencode-ai"),
            Some("opencode".to_string())
        );
        assert_eq!(
            command_name_from_mise_tool_key("npm:deepagents"),
            Some("deepagents".to_string())
        );
        assert_eq!(
            command_name_from_mise_tool_key("npm:pi"),
            Some("pi".to_string())
        );
        assert_eq!(
            command_name_from_mise_tool_key("npm:openai"),
            Some("openai".to_string())
        );
        assert_eq!(
            command_name_from_mise_tool_key("npm:some-tool-cli"),
            Some("some-tool".to_string())
        );
        assert_eq!(command_name_from_mise_tool_key("node"), None);
    }

    #[test]
    fn parse_zsh_alias_line_accepts_simple_alias() {
        assert_eq!(
            parse_zsh_alias_line("alias ocm='opencode -m ollama/gpt-mbx'"),
            Some((
                "ocm".to_string(),
                vec![
                    "opencode".to_string(),
                    "-m".to_string(),
                    "ollama/gpt-mbx".to_string()
                ]
            ))
        );
    }

    #[test]
    fn parse_zsh_alias_line_rejects_shell_control_alias() {
        assert_eq!(
            parse_zsh_alias_line("alias dotpull='cd \"$HOME/dotfiles\" && git pull --ff-only'"),
            None
        );
    }

    #[test]
    fn parse_fish_alias_line_accepts_simple_alias() {
        assert_eq!(
            parse_fish_alias_or_abbr_line("alias ghiss \"gh issue list\""),
            Some((
                "ghiss".to_string(),
                vec!["gh".to_string(), "issue".to_string(), "list".to_string()]
            ))
        );
    }

    #[test]
    fn parse_fish_abbr_line_accepts_simple_abbreviation() {
        assert_eq!(
            parse_fish_alias_or_abbr_line("abbr --add ocm opencode -m ollama/gpt-mbx"),
            Some((
                "ocm".to_string(),
                vec![
                    "opencode".to_string(),
                    "-m".to_string(),
                    "ollama/gpt-mbx".to_string()
                ]
            ))
        );
    }
}
