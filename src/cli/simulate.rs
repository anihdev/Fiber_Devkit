//! `fiber simulate` command adapter.
//! Owns the dry-run compatibility surface; the command delegates to the same
//! route analyzer as `fiber predict` and never performs live payments.

use std::io::{self, Write};
use std::path::PathBuf;

use clap::Args as ClapArgs;

use crate::route::analyzer::RouteAnalyzer;
use crate::{app_error, AppResult};

/// Run the dry-run simulation surface backed by route prediction.
#[derive(ClapArgs)]
#[command(after_help = "Example:
  fiber simulate node-1 node-2 1 --dry-run

This command is intentionally dry-run only. It delegates to the same analyzer as `fiber predict` and never sends a live payment.")]
pub struct Args {
    /// Source node name from `.fiber/config.toml`.
    pub from: String,
    /// Destination node name from `.fiber/config.toml`.
    pub to: String,
    /// Amount to evaluate, expressed in the selected asset's display unit.
    pub amount: String,
    /// Asset symbol to evaluate.
    #[arg(long, default_value = "CKB")]
    pub asset: String,
    /// Required safety flag: simulate never executes payments.
    #[arg(long)]
    pub dry_run: bool,
}

/// Runs route prediction through the legacy simulation-shaped command surface.
pub async fn execute(project_root: PathBuf, args: Args) -> AppResult<()> {
    if !args.dry_run {
        return Err(app_error(
            "`fiber simulate` is restricted to --dry-run; use scenario `pay` for live payments",
        ));
    }

    let prediction = RouteAnalyzer::new(project_root)
        .can_pay(&args.from, &args.to, &args.amount, &args.asset)
        .await?;
    println!("{}", serde_json::to_string_pretty(&prediction)?);
    io::stdout().flush()?;
    Ok(())
}
