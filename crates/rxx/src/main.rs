use anyhow::{Context, Result};
use clap::Parser;
use rx_core::{DirectRunRequest, ExecutionPlan, plan_direct_run};
use std::{
    path::PathBuf,
    process::{ExitStatus, Stdio, exit},
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
    let plan = plan_direct_run(&DirectRunRequest {
        script_path: cli.script,
        args: cli.args,
    })?;
    let status = execute_plan(&plan)?;
    exit_with_status(status);
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
