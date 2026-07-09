//! `fiber up` command adapter.
//! Starts the configured local FNN containers.

use std::path::PathBuf;

use clap::Args as ClapArgs;

use crate::network::manager::NetworkManager;
use crate::AppResult;

/// Start all configured local FNN containers.
#[derive(ClapArgs)]
#[command(after_help = "Example:
  fiber up

Starts the Dockerized FNN nodes from `.fiber/config.toml`, waits for `node_info`, then connects generated peers.")]
pub struct Args {}

/// Starts all configured FNN containers and waits for RPC readiness.
pub async fn execute(project_root: PathBuf, _args: Args) -> AppResult<()> {
    let manager = NetworkManager::new(project_root);
    manager.up().await
}
