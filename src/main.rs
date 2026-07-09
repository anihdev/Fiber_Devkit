//! CLI entry point for Fiber DevKit.
//! Owns command dispatch and process exit mapping; command behavior lives in
//! `src/cli`, not here.

mod cli;
mod config;
mod console;
mod diagnostic;
mod network;
mod reporter;
mod route;
mod rpc;
mod scenario;
mod tracer;
mod visibility;

use std::error::Error;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::cli::{
    ci, console as console_cmd, doctor, down, init, inspect, predict, report, reset,
    run as run_cmd, simulate, up, validate,
};

/// Shared fallible result type used across CLI, Docker, config, and RPC layers.
pub type AppResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// Creates a simple boxed error for human-facing command failures.
pub fn app_error(message: impl Into<String>) -> Box<dyn Error + Send + Sync> {
    Box::new(std::io::Error::other(message.into()))
}

#[derive(Parser)]
#[command(name = "fiber")]
#[command(version)]
#[command(
    about = "Fiber DevKit: Development infrastructure on Fiber Network (FNN), for network testing, diagnostics, route prediction, and CI scaffolding"
)]
#[command(
    long_about = "Fiber DevKit is a local development infrastructure CLI for Fiber Network. It creates a reproducible multi-node FNN Docker network, runs YAML scenarios, diagnoses payment failures, predicts route viability, emits report artifacts, and scaffolds CI smoke tests."
)]
#[command(after_help = "Common flows:
  fiber init --nodes 3 --template hub-spoke
  fiber up
  fiber inspect
  fiber console
  fiber run scenarios/network-smoke.yaml --report
  fiber doctor FIBER_LIQ_001 --explain
  fiber predict node-1 node-2 1 --cross-chain
  fiber down

Funded payment scenarios require testnet CKB on generated node keys; run `pnpm fund:nodes` after init/reset and before `fiber up`.")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Init(init::Args),
    Validate(validate::Args),
    Up(up::Args),
    Down(down::Args),
    Reset(reset::Args),
    Run(run_cmd::Args),
    Predict(predict::Args),
    Simulate(simulate::Args),
    Doctor(doctor::Args),
    Report(report::Args),
    Ci(ci::Args),
    Inspect(inspect::Args),
    Console(console_cmd::Args),
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> AppResult<()> {
    let cli = Cli::parse();
    let project_root = std::env::current_dir()?;

    match cli.command {
        Commands::Init(args) => init::execute(project_root, args).await,
        Commands::Validate(args) => validate::execute(project_root, args).await,
        Commands::Up(args) => up::execute(project_root, args).await,
        Commands::Down(args) => down::execute(project_root, args).await,
        Commands::Reset(args) => reset::execute(project_root, args).await,
        Commands::Run(args) => run_cmd::execute(project_root, args).await,
        Commands::Predict(args) => predict::execute(project_root, args).await,
        Commands::Simulate(args) => simulate::execute(project_root, args).await,
        Commands::Doctor(args) => doctor::execute(project_root, args).await,
        Commands::Report(args) => report::execute(project_root, args).await,
        Commands::Ci(args) => ci::execute(project_root, args).await,
        Commands::Inspect(args) => inspect::execute(project_root, args).await,
        Commands::Console(args) => console_cmd::execute(project_root, args).await,
    }
}
