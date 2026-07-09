//! `fiber inspect` command adapter.
//! Produces read-only local network visibility output without mutating node state.

use std::io::{self, Write};
use std::path::PathBuf;

use clap::Args as ClapArgs;

use crate::visibility::{inspect_project, InspectStatus, NodeInspection};
use crate::AppResult;

/// Show read-only node health, peer counts, and channel state.
#[derive(ClapArgs)]
#[command(after_help = "Examples:
  fiber inspect
  fiber inspect node-1 --channels
  fiber inspect --json

Reads `.fiber/config.toml` and calls `node_info`/`list_channels`. Unreachable nodes are shown and the command continues so partial network state remains visible.")]
pub struct Args {
    /// Optional configured node name, such as `node-1`.
    pub node: Option<String>,
    /// Include per-channel state for human-readable output.
    #[arg(long)]
    pub channels: bool,
    /// Emit structured JSON instead of human-readable rows.
    #[arg(long)]
    pub json: bool,
}

/// Reads configured nodes and prints their current health/channel state.
pub async fn execute(project_root: PathBuf, args: Args) -> AppResult<()> {
    let output = inspect_project(&project_root, args.node.as_deref()).await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        print_human(&output.nodes, args.channels);
    }
    io::stdout().flush()?;
    Ok(())
}

fn print_human(nodes: &[NodeInspection], include_channels: bool) {
    for node in nodes {
        match node.status {
            InspectStatus::Reachable => {
                println!(
                    "{}  reachable  {}  pubkey={}  peers={}  channels={}/{} ready",
                    node.name,
                    node.rpc_endpoint,
                    node.short_pubkey.as_deref().unwrap_or("unknown"),
                    node.peer_count
                        .map(|count| count.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    node.ready_channel_count,
                    node.total_channel_count
                );
                if include_channels {
                    for channel in &node.channels {
                        println!(
                            "  -> {}  state={}  enabled={}  local={}  remote={}",
                            channel.peer,
                            channel.state,
                            channel.enabled,
                            channel.local_balance.as_deref().unwrap_or("unknown"),
                            channel.remote_balance.as_deref().unwrap_or("unknown")
                        );
                    }
                }
            }
            InspectStatus::Unreachable => {
                println!("{}  unreachable  {}", node.name, node.rpc_endpoint);
                if let Some(error) = &node.error {
                    println!("  error: {error}");
                }
            }
        }
    }
}
