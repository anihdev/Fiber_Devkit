//! `fiber report` command adapter.
//! Regenerates Demo 5 artifacts from the most recent persisted scenario result.

use std::io::{self, Write};
use std::path::PathBuf;

use clap::{Args as ClapArgs, ValueEnum};

use crate::reporter::formats::Reporter;
use crate::AppResult;

/// Regenerate report artifacts from the latest scenario run.
#[derive(ClapArgs)]
#[command(after_help = "Examples:
  fiber run scenarios/network-smoke.yaml --report
  fiber report --format md
  fiber report --format json

Regenerates `report.md`, `logs.json`, `trace.json`, and `last-run.json` from the latest
persisted run; it does not rerun scenarios or query live nodes. `--format md` prints the
path to `report.md`. `--format json` prints the path to `logs.json`. Both choices regenerate
the complete artifact set.")]
pub struct Args {
    /// Select the artifact path to print: `md` for report.md or `json` for logs.json.
    #[arg(long, value_enum, default_value = "md")]
    pub format: ReportFormat,
}

/// User-facing report format selector.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ReportFormat {
    Md,
    Json,
}

/// Regenerates report artifacts from `.fiber/output/last-run.json`.
pub async fn execute(project_root: PathBuf, args: Args) -> AppResult<()> {
    let artifacts = Reporter::new(project_root).write_from_last_run()?;
    match args.format {
        ReportFormat::Md => println!("{}", artifacts.report_md.display()),
        ReportFormat::Json => println!("{}", artifacts.logs_json.display()),
    }
    io::stdout().flush()?;
    Ok(())
}
