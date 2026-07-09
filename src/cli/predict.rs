//! `fiber predict` command adapter.
//! Parses route prediction arguments and renders the JSON response.

use std::io::{self, Write};
use std::path::PathBuf;

use clap::Args as ClapArgs;

use crate::route::analyzer::RouteAnalyzer;
use crate::AppResult;

/// Score route confidence without sending a payment.
#[derive(ClapArgs)]
#[command(after_help = "Examples:
  fiber predict node-1 node-2 1
  fiber predict node-1 node-2 1 --cross-chain

Uses configured node names from `.fiber/config.toml`. The cross-chain view reports CCH availability/mechanism honestly and does not fabricate live CCH order probability.")]
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
    /// Include CCH bridge availability/mechanism comparison beside native Fiber.
    #[arg(long)]
    pub cross_chain: bool,
}

/// Runs native route prediction and optionally includes the CCH bridge statement.
pub async fn execute(project_root: PathBuf, args: Args) -> AppResult<()> {
    let analyzer = RouteAnalyzer::new(project_root);
    if args.cross_chain {
        let comparison = analyzer
            .compare_routes(&args.from, &args.to, &args.amount, &args.asset)
            .await?;
        println!("{}", serde_json::to_string_pretty(&comparison)?);
    } else {
        let prediction = analyzer
            .can_pay(&args.from, &args.to, &args.amount, &args.asset)
            .await?;
        println!("{}", serde_json::to_string_pretty(&prediction)?);
    }
    io::stdout().flush()?;
    Ok(())
}
