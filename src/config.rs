//! Project configuration model for Fiber DevKit.
//! Serializes `.fiber/config.toml` and keeps deterministic defaults for local nodes.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::{app_error, AppResult};

/// Directory where generated network state and node config are stored.
pub const FIBER_DIR: &str = ".fiber";
/// Official Fiber Docker repository pinned for the hackathon demos.
pub const IMAGE_REPOSITORY: &str = "ghcr.io/nervosnetwork/fiber";
/// Canonical Docker image tag. Git release tags keep the leading `v`; container tags do not.
pub const IMAGE_TAG: &str = "0.9.0-rc5";
/// Fully-qualified FNN image written to generated config.
pub const IMAGE: &str = "ghcr.io/nervosnetwork/fiber:0.9.0-rc5";
/// Docker bridge network shared by all local FNN containers.
pub const DOCKER_NETWORK: &str = "fiber-devkit";
/// Default CKB testnet RPC endpoint used by generated FNN configs.
pub const DEFAULT_CKB_RPC_URL: &str = "http://testnet.ckb.dev/";
/// FNN RPC port inside each container.
pub const INTERNAL_RPC_PORT: u16 = 8227;
/// FNN P2P port inside each container.
pub const INTERNAL_P2P_PORT: u16 = 8228;
/// Development-only password for local generated node keys.
pub const DEFAULT_SECRET_PASSWORD: &str = "fiber-devkit-local";

/// Supported local network topologies for Demo 1 and Demo 2.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Topology {
    HubSpoke,
    Mesh,
}

impl fmt::Display for Topology {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HubSpoke => write!(formatter, "hub-spoke"),
            Self::Mesh => write!(formatter, "mesh"),
        }
    }
}

/// MVP node role used when rendering FNN config templates.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeTemplate {
    Hub,
    Leaf,
}

impl fmt::Display for NodeTemplate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hub => write!(formatter, "hub"),
            Self::Leaf => write!(formatter, "leaf"),
        }
    }
}

/// Serialized project-level config written to `.fiber/config.toml`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DevkitConfig {
    pub version: u8,
    pub image: String,
    pub ckb_rpc_url: String,
    pub topology: Topology,
    pub nodes: Vec<NodeConfig>,
}

/// Serialized config for one generated FNN Docker node.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NodeConfig {
    pub name: String,
    pub template: NodeTemplate,
    pub container_name: String,
    pub rpc_port: u16,
    pub p2p_port: u16,
    pub data_dir: PathBuf,
    pub config_path: PathBuf,
}

impl DevkitConfig {
    /// Returns the canonical config path for a project root.
    pub fn path(project_root: &Path) -> PathBuf {
        project_root.join(FIBER_DIR).join("config.toml")
    }

    /// Reads and validates `.fiber/config.toml` for the current schema version.
    pub fn read_from_project(project_root: &Path) -> AppResult<Self> {
        let path = Self::path(project_root);
        let raw = fs::read_to_string(&path)?;
        let config: Self = toml::from_str(&raw)?;
        if config.version != 1 {
            return Err(app_error(format!(
                "Unsupported config version `{}` in {}",
                config.version,
                path.display()
            )));
        }
        Ok(config)
    }

    /// Writes this config to `.fiber/config.toml` using stable TOML formatting.
    pub fn write_to_project(&self, project_root: &Path) -> AppResult<()> {
        let path = Self::path(project_root);
        let rendered = toml::to_string_pretty(self)?;
        fs::write(path, rendered)?;
        Ok(())
    }
}

impl NodeConfig {
    /// Public localhost JSON-RPC endpoint exposed by Docker port bindings.
    pub fn rpc_endpoint(&self) -> String {
        format!("http://127.0.0.1:{}/", self.rpc_port)
    }

    /// Absolute host path for this node's generated data directory.
    pub fn absolute_data_dir(&self, project_root: &Path) -> PathBuf {
        project_root.join(&self.data_dir)
    }

    /// Absolute host path for this node's generated FNN YAML config.
    pub fn absolute_config_path(&self, project_root: &Path) -> PathBuf {
        project_root.join(&self.config_path)
    }
}

/// Builds deterministic node names, templates, and ports for a topology.
pub fn default_node_configs(topology: Topology, nodes: usize) -> Vec<NodeConfig> {
    (0..nodes)
        .map(|index| {
            let ordinal = index + 1;
            let name = format!("node-{ordinal}");
            let template = match topology {
                Topology::HubSpoke if index == 0 => NodeTemplate::Hub,
                Topology::HubSpoke => NodeTemplate::Leaf,
                Topology::Mesh => NodeTemplate::Hub,
            };
            // Keep ports deterministic so generated scenarios can refer to stable node names.
            let rpc_port = 8227 + (index as u16 * 2);
            let p2p_port = 8228 + (index as u16 * 2);
            NodeConfig {
                name: name.clone(),
                template,
                container_name: format!("fiber-devkit-{name}"),
                rpc_port,
                p2p_port,
                data_dir: PathBuf::from(format!("{FIBER_DIR}/nodes/{name}")),
                config_path: PathBuf::from(format!("{FIBER_DIR}/nodes/{name}/config.yml")),
            }
        })
        .collect()
}
