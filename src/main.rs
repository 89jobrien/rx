use anyhow::Result;
use clap::{Parser, Subcommand};
use rx::{InstallRequest, install};
use std::{env, path::PathBuf};

#[derive(Debug, Parser)]
#[command(
    name = "rx",
    about = "Install rust-script scripts from local or remote sources"
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Install {
            source,
            install_dir,
        } => {
            let report = install(&InstallRequest {
                source,
                install_dir,
            })?;

            for script in &report.installed {
                println!(
                    "installed {} -> {}",
                    script.source,
                    script.destination.display()
                );
            }

            if !report.skipped.is_empty() {
                eprintln!("skipped non-rust-script files:");
                for item in &report.skipped {
                    eprintln!("  {item}");
                }
            }
        }
    }

    Ok(())
}

fn default_install_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".local/bin"))
        .unwrap_or_else(|| PathBuf::from("."))
}
