//! `fiber ci` command adapter.
//! Owns GitHub Actions scaffold generation for Demo 5; it does not run CI or
//! contact GitHub APIs.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use clap::{Args as ClapArgs, Subcommand};

use crate::{app_error, AppResult};

/// Generate reusable CI scaffolding.
#[derive(ClapArgs)]
#[command(after_help = "Example:
  fiber ci init

Writes a GitHub Actions workflow that builds DevKit, runs Rust/TypeScript checks, starts the local Fiber network, and executes the unfunded smoke scenario.")]
pub struct Args {
    #[command(subcommand)]
    pub command: CiCommand,
}

/// CI subcommands.
#[derive(Subcommand)]
pub enum CiCommand {
    /// Write `.github/workflows/fiber-ci.yml`.
    Init,
}

/// Dispatches CI scaffold commands.
pub async fn execute(project_root: PathBuf, args: Args) -> AppResult<()> {
    match args.command {
        CiCommand::Init => init(project_root),
    }
}

fn init(project_root: PathBuf) -> AppResult<()> {
    let workflow_dir = project_root.join(".github").join("workflows");
    let workflow_path = workflow_dir.join("fiber-ci.yml");
    fs::create_dir_all(&workflow_dir)?;
    if workflow_path.exists() {
        return Err(app_error(format!(
            "{} already exists; remove it before regenerating",
            workflow_path.display()
        )));
    }

    fs::write(&workflow_path, workflow_yaml())?;
    println!("{}", workflow_path.display());
    io::stdout().flush()?;
    Ok(())
}

fn workflow_yaml() -> &'static str {
    r#"name: Fiber DevKit

on:
  push:
  pull_request:

jobs:
  fiber-devkit:
    runs-on: ubuntu-latest
    timeout-minutes: 45
    steps:
      - name: Checkout
        uses: actions/checkout@v5

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Cache Rust build
        uses: Swatinem/rust-cache@v2

      - name: Install Node
        uses: actions/setup-node@v5
        with:
          node-version: 22
          package-manager-cache: false

      - name: Enable pnpm
        run: corepack enable

      - name: Install support dependencies
        run: pnpm install --frozen-lockfile

      - name: Build fiber
        timeout-minutes: 25
        run: cargo build --locked

      - name: Typecheck support scripts
        run: pnpm typecheck

      - name: Check formatting
        run: cargo fmt --check

      - name: Run tests
        run: cargo test --locked

      - name: Run clippy
        run: cargo clippy --locked -- -D warnings

      - name: Initialize DevKit config
        run: target/debug/fiber init --nodes 3 --template hub-spoke

      - name: Pull pinned FNN image
        run: docker pull ghcr.io/nervosnetwork/fiber:0.9.0-rc5

      - name: Validate generated config
        run: target/debug/fiber validate

      - name: Run unfunded network smoke scenario
        run: |
          trap 'target/debug/fiber down' EXIT
          target/debug/fiber up
          target/debug/fiber run scenarios/network-smoke.yaml --report
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_uses_demo5_commands() {
        let yaml = workflow_yaml();

        assert!(yaml.contains("cargo test --locked"));
        assert!(yaml.contains("actions/checkout@v5"));
        assert!(yaml.contains("actions/setup-node@v5"));
        assert!(yaml.contains("Swatinem/rust-cache@v2"));
        assert!(yaml.contains("package-manager-cache: false"));
        assert!(yaml.contains("timeout-minutes: 25"));
        assert!(yaml.contains("pnpm install --frozen-lockfile"));
        assert!(yaml.contains("pnpm typecheck"));
        assert!(yaml.contains("docker pull ghcr.io/nervosnetwork/fiber:0.9.0-rc5"));
        assert!(yaml.contains("target/debug/fiber run scenarios/network-smoke.yaml --report"));
        assert!(yaml.contains("target/debug/fiber down"));
    }
}
