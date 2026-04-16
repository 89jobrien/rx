use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use rx_core::{
    ExecutionPlan, InstallRequest, RunRequest, format_registry_entry, install, list_installed,
    plan_installed_run,
};
use rx_registry_json::{JsonRegistryStore, ReqwestFetcher, default_paths};
use std::{
    path::PathBuf,
    process::{ExitStatus, Stdio, exit},
};

#[derive(Debug, Parser)]
#[command(
    name = "rx",
    about = "Install compatible scripts from local or remote sources"
)]
struct Cli {
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();

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
            let status = execute_plan(&plan)?;
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

fn execute_plan(plan: &ExecutionPlan) -> Result<ExitStatus> {
    std::process::Command::new(&plan.program)
        .args(&plan.args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("running {}", plan.program))
}

fn exit_with_status(status: ExitStatus) -> ! {
    exit(status.code().unwrap_or(1));
}
