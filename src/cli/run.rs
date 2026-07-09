//! `fiber run` command adapter.
//! Parses scenario files and renders run output for the CLI.

use std::io::{self, Write};
use std::path::PathBuf;

use clap::Args as ClapArgs;

use crate::reporter::formats::Reporter;
use crate::scenario::parser::ScenarioParser;
use crate::scenario::runner::ScenarioRunner;
use crate::{app_error, AppResult};

/// Execute a scenario file against the local network.
#[derive(ClapArgs)]
#[command(after_help = "Examples:
  fiber run scenarios/network-smoke.yaml
  fiber run scenarios/basic-payment.yaml --report

Prints one JSON object per step plus a final summary. `--report` also writes human and machine artifacts under `.fiber/output/`.")]
pub struct Args {
    /// Path to a Fiber DevKit scenario YAML file.
    pub scenario: PathBuf,
    /// Write `.fiber/output/report.md`, `logs.json`, and `trace.json`.
    #[arg(long)]
    pub report: bool,
}

/// Parses a scenario, executes it against the local network, and prints JSON lines.
pub async fn execute(project_root: PathBuf, args: Args) -> AppResult<()> {
    let scenario = ScenarioParser::parse(&args.scenario)?;
    let runner = ScenarioRunner::new(project_root.clone());
    let result = runner.run(scenario).await?;

    for step in &result.steps {
        println!("{}", serde_json::to_string(step)?);
    }
    println!("{}", serde_json::to_string(&result.summary())?);
    io::stdout().flush()?;

    let reporter = Reporter::new(project_root);
    reporter.persist_last_run(&result)?;
    if args.report {
        let artifacts = reporter.write_all(&result)?;
        eprintln!(
            "report artifacts written to {}",
            artifacts.output_dir.display()
        );
    }

    if result.passed {
        Ok(())
    } else {
        Err(app_error("scenario failed"))
    }
}
