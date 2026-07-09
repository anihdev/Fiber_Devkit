//! `fiber init` command adapter.
//! Parses arguments for network scaffolding and initialization.

use std::path::PathBuf;

use clap::Args as ClapArgs;

use crate::config::Topology;
use crate::network::manager::{InitOptions, NetworkManager};
use crate::AppResult;

/// Generate `.fiber/` config, node files, and Docker setup.
#[derive(ClapArgs)]
#[command(after_help = "Examples:
  fiber init
  fiber init --nodes 3 --template hub-spoke
  fiber init --nodes 4 --template mesh

Creates `.fiber/config.toml`, per-node FNN config/key files, and `.fiber/docker-compose.yml`.")]
pub struct Args {
    /// Number of local FNN nodes to generate.
    #[arg(long, default_value_t = 3)]
    pub nodes: usize,
    /// Local topology template to generate.
    #[arg(long, value_enum, default_value_t = Topology::HubSpoke)]
    pub template: Topology,
}

/// Initializes `.fiber/` for a reproducible local multi-node network.
pub async fn execute(project_root: PathBuf, args: Args) -> AppResult<()> {
    let manager = NetworkManager::new(project_root);
    let config = manager.init(InitOptions {
        nodes: args.nodes,
        topology: args.template,
    })?;

    println!(
        "Initialized Fiber network: {} nodes using {} topology",
        config.nodes.len(),
        config.topology
    );
    println!("Wrote .fiber/config.toml");
    println!("Wrote .fiber/docker-compose.yml");
    Ok(())
}
