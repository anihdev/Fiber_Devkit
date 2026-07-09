//! `fiber validate` pre-flight checks.
//! Validates project configuration and reports issues without mutating generated files.

use std::collections::HashSet;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bollard::Docker;
use clap::Args as ClapArgs;

use crate::config::{DevkitConfig, IMAGE};
use crate::{app_error, AppResult};

/// Check Docker, config, ports, image cache, and optional live RPC.
#[derive(ClapArgs)]
#[command(after_help = "Examples:
  fiber validate
  fiber up
  fiber validate --live

Without --live, ports are expected to be free before `fiber up`. With --live, DevKit checks the running FNN RPC endpoints instead.")]
pub struct Args {
    /// Check running node RPC endpoints instead of expecting ports to be free.
    #[arg(long)]
    pub live: bool,
}

enum CheckStatus {
    Pass,
    Fail,
    Warn,
}

struct Check {
    name: &'static str,
    status: CheckStatus,
    message: String,
}

impl Check {
    fn pass(name: &'static str, message: impl Into<String>) -> Self {
        Self {
            name,
            status: CheckStatus::Pass,
            message: message.into(),
        }
    }

    fn fail(name: &'static str, message: impl Into<String>) -> Self {
        Self {
            name,
            status: CheckStatus::Fail,
            message: message.into(),
        }
    }

    fn warn(name: &'static str, message: impl Into<String>) -> Self {
        Self {
            name,
            status: CheckStatus::Warn,
            message: message.into(),
        }
    }
}

/// Executes all configured pre-flight checks and fails if any hard check fails.
pub async fn execute(project_root: PathBuf, args: Args) -> AppResult<()> {
    let mut checks = Vec::new();

    // Config is validated first because later checks depend on generated ports and paths.
    let config = match DevkitConfig::read_from_project(&project_root) {
        Ok(config) => {
            checks.push(Check::pass(
                "config",
                ".fiber/config.toml is present and parseable",
            ));
            Some(config)
        }
        Err(err) => {
            checks.push(Check::fail(
                "config",
                format!("Cannot read .fiber/config.toml: {err}. Run `fiber init`."),
            ));
            None
        }
    };

    // Docker reachability is isolated from image checks so users get both diagnostics.
    let docker = match Docker::connect_with_local_defaults() {
        Ok(docker) => match docker.ping().await {
            Ok(_) => {
                checks.push(Check::pass("docker", "Docker daemon is reachable"));
                Some(docker)
            }
            Err(err) => {
                checks.push(Check::fail(
                    "docker",
                    format!("Docker daemon is not running or not reachable: {err}"),
                ));
                None
            }
        },
        Err(err) => {
            checks.push(Check::fail(
                "docker",
                format!("Docker client could not be created: {err}"),
            ));
            None
        }
    };

    if let Some(config) = config.as_ref() {
        checks.push(validate_config_shape(config));
        checks.push(validate_generated_files(&project_root, config));
        if args.live {
            checks.push(Check::pass(
                "ports",
                "Skipped free-port check because --live expects nodes to be running",
            ));
        } else {
            // Offline validation expects ports to be free before `fiber up` binds them.
            checks.push(validate_ports(config));
        }
        checks.push(validate_ckb_rpc(config).await);

        if args.live {
            // Live mode verifies running FNN JSON-RPC endpoints instead of free ports.
            checks.push(validate_live_rpc(config).await);
        }
    }

    if let Some(docker) = docker.as_ref() {
        checks.push(validate_image_cached(docker).await);
    }

    print_checks(&checks);

    if checks
        .iter()
        .any(|check| matches!(check.status, CheckStatus::Fail))
    {
        return Err(app_error("validation failed"));
    }

    Ok(())
}

fn validate_config_shape(config: &DevkitConfig) -> Check {
    if config.nodes.is_empty() {
        return Check::fail("config valid", "Config must define at least one node");
    }

    let mut names = HashSet::new();
    let mut ports = HashSet::new();
    for node in &config.nodes {
        if !names.insert(&node.name) {
            return Check::fail(
                "config valid",
                format!("Duplicate node name `{}` in config", node.name),
            );
        }
        if !ports.insert(node.rpc_port) {
            return Check::fail(
                "config valid",
                format!("Duplicate RPC port `{}` in config", node.rpc_port),
            );
        }
        if !ports.insert(node.p2p_port) {
            return Check::fail(
                "config valid",
                format!("Duplicate P2P port `{}` in config", node.p2p_port),
            );
        }
    }

    Check::pass(
        "config valid",
        format!(
            "{} nodes configured with unique names and ports",
            config.nodes.len()
        ),
    )
}

fn validate_generated_files(project_root: &Path, config: &DevkitConfig) -> Check {
    let compose_path = project_root.join(".fiber/docker-compose.yml");
    if !compose_path.exists() {
        return Check::fail(
            "docker setup",
            "Missing .fiber/docker-compose.yml. Run `fiber init` again.",
        );
    }

    for node in &config.nodes {
        if !project_root.join(&node.config_path).exists() {
            return Check::fail(
                "docker setup",
                format!("Missing generated FNN config for node `{}`", node.name),
            );
        }
    }

    Check::pass(
        "docker setup",
        "Generated Docker and FNN config files exist",
    )
}

fn validate_ports(config: &DevkitConfig) -> Check {
    for node in &config.nodes {
        for (label, port) in [("RPC", node.rpc_port), ("P2P", node.p2p_port)] {
            if let Err(err) = TcpListener::bind(("127.0.0.1", port)) {
                return Check::fail(
                    "ports",
                    format!(
                        "{} port {} for node `{}` is already in use. Stop the conflicting process or run `fiber down`.",
                        label, port, node.name
                    ),
                )
                .with_detail(err.to_string());
            }
        }
    }

    Check::pass(
        "ports",
        "Configured localhost RPC and P2P ports are available",
    )
}

async fn validate_image_cached(docker: &Docker) -> Check {
    match docker.inspect_image(IMAGE).await {
        Ok(_) => Check::pass("image", format!("Docker image `{IMAGE}` is cached")),
        Err(err) => Check::fail(
            "image",
            format!("Docker image `{IMAGE}` is not cached or cannot be inspected: {err}. Run `fiber up` to pull the pinned image."),
        ),
    }
}

async fn validate_ckb_rpc(config: &DevkitConfig) -> Check {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            return Check::warn(
                "ckb rpc",
                format!("Could not create HTTP client for CKB RPC check: {err}"),
            );
        }
    };

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "get_tip_header",
        "params": [],
        "id": 1
    });

    match client.post(&config.ckb_rpc_url).json(&body).send().await {
        Ok(response) if response.status().is_success() => Check::pass(
            "ckb rpc",
            format!("CKB RPC reachable at {}", config.ckb_rpc_url),
        ),
        Ok(response) => Check::fail(
            "ckb rpc",
            format!(
                "CKB RPC {} returned HTTP {}",
                config.ckb_rpc_url,
                response.status()
            ),
        ),
        Err(err) => Check::fail(
            "ckb rpc",
            format!("CKB RPC {} is not reachable: {err}", config.ckb_rpc_url),
        ),
    }
}

async fn validate_live_rpc(config: &DevkitConfig) -> Check {
    let client = reqwest::Client::new();
    for node in &config.nodes {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "node_info",
            "params": [],
            "id": 1
        });
        let endpoint = node.rpc_endpoint();
        match client.post(&endpoint).json(&body).send().await {
            Ok(response) if response.status().is_success() => {}
            Ok(response) => {
                return Check::fail(
                    "live rpc",
                    format!("{} returned HTTP {}", endpoint, response.status()),
                );
            }
            Err(err) => {
                return Check::fail("live rpc", format!("{} is not reachable: {err}", endpoint));
            }
        }
    }

    Check::pass("live rpc", "All configured nodes answer node_info")
}

fn print_checks(checks: &[Check]) {
    for check in checks {
        let marker = match check.status {
            CheckStatus::Pass => "PASS",
            CheckStatus::Fail => "FAIL",
            CheckStatus::Warn => "WARN",
        };
        println!("[{marker}] {} - {}", check.name, check.message);
    }
}

trait WithDetail {
    fn with_detail(self, detail: String) -> Self;
}

impl WithDetail for Check {
    fn with_detail(mut self, detail: String) -> Self {
        self.message = format!("{} ({detail})", self.message);
        self
    }
}
