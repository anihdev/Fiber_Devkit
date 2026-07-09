//! `fiber reset` command adapter.
//! Exposes teardown plus reinitialization without owning cleanup policy.

use std::path::PathBuf;

use clap::Args as ClapArgs;

use crate::network::manager::NetworkManager;
use crate::AppResult;

/// Recreate the generated local network state.
#[derive(ClapArgs)]
#[command(after_help = "Example:
  fiber reset

Stops managed containers and recreates `.fiber` using the previous generated topology when possible. Funded payment scenarios need `pnpm fund:nodes` again after reset.")]
pub struct Args {}

/// Recreates `.fiber/` using the previous topology when possible.
pub async fn execute(project_root: PathBuf, _args: Args) -> AppResult<()> {
    let manager = NetworkManager::new(project_root);
    manager.reset().await
}
