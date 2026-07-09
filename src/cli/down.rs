//! `fiber down` command adapter.
//! Stops local FNN containers without removing generated scenario or config files.

use std::path::PathBuf;

use clap::Args as ClapArgs;

use crate::network::manager::NetworkManager;
use crate::AppResult;

/// Stop and remove the local FNN containers.
#[derive(ClapArgs)]
#[command(after_help = "Example:
  fiber down

Stops containers managed by the current DevKit config. Generated `.fiber` config, keys, reports, and scenario files are preserved.")]
pub struct Args {}

/// Stops and removes all containers managed by the current project config.
pub async fn execute(project_root: PathBuf, _args: Args) -> AppResult<()> {
    let manager = NetworkManager::new(project_root);
    manager.down().await
}
