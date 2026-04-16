use anyhow::Result;
use clap::Parser;
use rx::{DirectRunRequest, run_direct};
use std::{
    path::PathBuf,
    process::{ExitStatus, exit},
};

#[derive(Debug, Parser)]
#[command(
    name = "rxx",
    about = "Execute compatible scripts directly",
    trailing_var_arg = true
)]
struct Cli {
    script: PathBuf,
    #[arg(allow_hyphen_values = true)]
    args: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let status = run_direct(&DirectRunRequest {
        script_path: cli.script,
        args: cli.args,
    })?;
    exit_with_status(status);
}

fn exit_with_status(status: ExitStatus) -> ! {
    exit(status.code().unwrap_or(1));
}
