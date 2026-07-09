//! `fiber doctor` command adapter.
//! Accepts logs, taxonomy codes, or raw error text and renders diagnostic output.

use std::io::{self, Write};
use std::path::PathBuf;

use clap::Args as ClapArgs;
use serde_json::json;

use crate::diagnostic::engine::{DiagnosisReport, DiagnosticEngine};
use crate::{app_error, AppResult};

/// Diagnose Fiber failures from logs, taxonomy codes, or raw error text.
#[derive(ClapArgs)]
#[command(after_help = "Examples:
  fiber doctor /tmp/fiber-low-liquidity.jsonl --explain
  fiber doctor FIBER_LIQ_001 --explain
  fiber doctor \"insufficient outbound liquidity\" --explain

Accepts scenario JSONL logs, known taxonomy codes, or raw FNN/RPC error text. Payment-hash lookup is roadmap work.")]
pub struct Args {
    /// Scenario JSONL log file path, taxonomy code, or raw error text.
    ///
    /// Payment-hash lookup is outside the Demo 3 MVP because it needs per-node
    /// FNN payment history queries.
    pub input: String,
    /// Print a narrative explanation instead of JSON.
    #[arg(long)]
    pub explain: bool,
}

/// Diagnoses a scenario log file, known taxonomy code, or raw error string.
pub async fn execute(_project_root: PathBuf, args: Args) -> AppResult<()> {
    let engine = DiagnosticEngine::new();
    let input_path = PathBuf::from(&args.input);
    let reports = if input_path.exists() {
        engine.parse_log_file(&input_path)?
    } else {
        vec![engine.diagnose_text(&args.input)]
    };

    if reports.is_empty() {
        return Err(app_error("no diagnosable Fiber failures found in input"));
    }

    if args.explain {
        print_explanations(&engine, &reports);
    } else {
        print_json(&reports)?;
    }
    io::stdout().flush()?;
    Ok(())
}

fn print_explanations(engine: &DiagnosticEngine, reports: &[DiagnosisReport]) {
    for (index, report) in reports.iter().enumerate() {
        if index > 0 {
            println!();
        }
        println!("{}", engine.explain(report));
    }
}

fn print_json(reports: &[DiagnosisReport]) -> AppResult<()> {
    if reports.len() == 1 {
        println!("{}", serde_json::to_string_pretty(&reports[0])?);
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "diagnoses": reports }))?
        );
    }
    Ok(())
}
