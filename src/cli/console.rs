//! `fiber console` command adapter.
//! Starts a read-only localhost browser console over existing DevKit data.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;

use clap::Args as ClapArgs;

use crate::console::server;
use crate::AppResult;

/// Serve the read-only local browser console.
#[derive(ClapArgs)]
#[command(after_help = "Examples:
  fiber console
  fiber console --port 7717
  fiber console --open

Serves a GET-only localhost UI over existing DevKit JSON contracts. The console is read-only and never starts nodes, runs scenarios, opens channels, funds keys, or sends payments.")]
pub struct Args {
    /// Localhost port for the console HTTP server.
    #[arg(long, default_value_t = 7717)]
    pub port: u16,
    /// Best-effort open of the local console URL in a browser.
    #[arg(long)]
    pub open: bool,
}

/// Starts the read-only console server and keeps it running until interrupted.
pub async fn execute(project_root: PathBuf, args: Args) -> AppResult<()> {
    let server = server::bind(args.port).await?;
    let url = server.url();
    println!("Fiber console serving read-only local data at {url}");
    if args.open {
        open_browser(&url);
    }
    io::stdout().flush()?;
    server.serve(project_root).await
}

fn open_browser(url: &str) {
    let result = if cfg!(target_os = "macos") {
        Command::new("open").arg(url).spawn()
    } else if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/C", "start", "", url]).spawn()
    } else {
        Command::new("xdg-open").arg(url).spawn()
    };

    if let Err(err) = result {
        eprintln!("Could not open browser automatically: {err}. Open {url} manually.");
    }
}
