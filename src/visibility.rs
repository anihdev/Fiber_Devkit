//! Shared read-only network visibility collection.
//! Provides inspect-style node and channel data for `fiber inspect` and the local console.

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

use crate::config::{DevkitConfig, NodeConfig};
use crate::rpc::client::FiberRpc;
use crate::rpc::types::{Channel, NodeInfo};
use crate::AppResult;

/// JSON envelope returned by `fiber inspect --json` and `/api/nodes`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectOutput {
    pub nodes: Vec<NodeInspection>,
}

/// Per-node visibility result collected from configured FNN RPC endpoints.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInspection {
    pub name: String,
    pub status: InspectStatus,
    pub rpc_endpoint: String,
    pub pubkey: Option<String>,
    pub short_pubkey: Option<String>,
    pub peer_count: Option<u64>,
    pub ready_channel_count: usize,
    pub total_channel_count: usize,
    pub channels: Vec<ChannelInspection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Reachability status for an inspected node.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InspectStatus {
    Reachable,
    Unreachable,
}

/// Per-channel visibility result for human and JSON output.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelInspection {
    pub peer: String,
    pub peer_pubkey: Option<String>,
    pub state: String,
    pub enabled: bool,
    pub local_balance: Option<String>,
    pub remote_balance: Option<String>,
}

/// Reads `.fiber/config.toml`, inspects configured nodes, and optionally narrows by name.
pub async fn inspect_project(
    project_root: &Path,
    requested_node: Option<&str>,
) -> AppResult<InspectOutput> {
    let config = DevkitConfig::read_from_project(project_root)?;
    Ok(inspect_config(&config, requested_node).await)
}

/// Inspects every configured node using the current channel view exposed by FNN.
pub async fn inspect_config(config: &DevkitConfig, requested_node: Option<&str>) -> InspectOutput {
    let mut nodes = inspect_configured_nodes(config).await;
    attach_peer_aliases(&mut nodes);

    InspectOutput {
        nodes: select_nodes(nodes, requested_node),
    }
}

async fn inspect_configured_nodes(config: &DevkitConfig) -> Vec<NodeInspection> {
    let mut results = Vec::new();
    for node in &config.nodes {
        results.push(inspect_configured_node(node).await);
    }
    results
}

async fn inspect_configured_node(node: &NodeConfig) -> NodeInspection {
    let endpoint = node.rpc_endpoint();
    let rpc = match FiberRpc::new(&endpoint) {
        Ok(rpc) => rpc,
        Err(err) => return unreachable_node(&node.name, endpoint, err.to_string()),
    };

    let info = match rpc.node_info().await {
        Ok(info) => info,
        Err(err) => return unreachable_node(&node.name, endpoint, err.to_string()),
    };

    match rpc.list_channels().await {
        Ok(channels) => reachable_node(node, endpoint, info, channels),
        Err(err) => {
            let mut node = unreachable_node(&node.name, endpoint, err.to_string());
            node.pubkey = Some(info.pubkey.clone());
            node.short_pubkey = Some(short_pubkey(&info.pubkey));
            node.peer_count = info.peers_count;
            node
        }
    }
}

fn reachable_node(
    node: &NodeConfig,
    endpoint: String,
    info: NodeInfo,
    channels: Vec<Channel>,
) -> NodeInspection {
    let ready_channel_count = channels
        .iter()
        .filter(|channel| channel.state_name.as_deref() == Some("ChannelReady"))
        .count();
    let total_channel_count = channels.len();
    NodeInspection {
        name: node.name.clone(),
        status: InspectStatus::Reachable,
        rpc_endpoint: endpoint,
        pubkey: Some(info.pubkey.clone()),
        short_pubkey: Some(short_pubkey(&info.pubkey)),
        peer_count: info.peers_count,
        ready_channel_count,
        total_channel_count,
        channels: channels.iter().map(channel_inspection).collect(),
        error: None,
    }
}

fn unreachable_node(name: &str, endpoint: String, error: String) -> NodeInspection {
    NodeInspection {
        name: name.to_string(),
        status: InspectStatus::Unreachable,
        rpc_endpoint: endpoint,
        pubkey: None,
        short_pubkey: None,
        peer_count: None,
        ready_channel_count: 0,
        total_channel_count: 0,
        channels: Vec::new(),
        error: Some(error),
    }
}

fn channel_inspection(channel: &Channel) -> ChannelInspection {
    let peer_pubkey = channel.pubkey.clone();
    ChannelInspection {
        peer: peer_pubkey
            .as_deref()
            .map(short_pubkey)
            .unwrap_or_else(|| "unknown".to_string()),
        peer_pubkey,
        state: channel
            .state_name
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        enabled: channel.enabled.unwrap_or(true),
        local_balance: channel.local_balance.map(format_ckb),
        remote_balance: channel.remote_balance.map(format_ckb),
    }
}

fn attach_peer_aliases(nodes: &mut [NodeInspection]) {
    let aliases = nodes
        .iter()
        .filter_map(|node| {
            node.pubkey
                .as_ref()
                .map(|pubkey| (pubkey.clone(), node.name.clone()))
        })
        .collect::<HashMap<_, _>>();

    for node in nodes {
        for channel in &mut node.channels {
            if let Some(alias) = channel
                .peer_pubkey
                .as_ref()
                .and_then(|pubkey| aliases.get(pubkey))
            {
                channel.peer = alias.clone();
            }
        }
    }
}

fn select_nodes(nodes: Vec<NodeInspection>, requested: Option<&str>) -> Vec<NodeInspection> {
    let Some(requested) = requested else {
        return nodes;
    };

    let mut matches = nodes
        .into_iter()
        .filter(|node| node.name == requested)
        .collect::<Vec<_>>();
    if matches.is_empty() {
        matches.push(unreachable_node(
            requested,
            "not configured".to_string(),
            format!("node `{requested}` is not present in .fiber/config.toml"),
        ));
    }
    matches
}

fn short_pubkey(pubkey: &str) -> String {
    if pubkey.len() <= 16 {
        return pubkey.to_string();
    }
    format!("{}...{}", &pubkey[..10], &pubkey[pubkey.len() - 6..])
}

fn format_ckb(shannons: u128) -> String {
    let whole = shannons / 100_000_000;
    let fraction = shannons % 100_000_000;
    if fraction == 0 {
        format!("{whole} CKB")
    } else {
        let fraction = format!("{fraction:08}").trim_end_matches('0').to_string();
        format!("{whole}.{fraction} CKB")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortens_pubkey_to_required_shape() {
        assert_eq!(
            short_pubkey("02720e4bf5c9acc7e5628c8b5ab9e499b539e453bd51d8d95681861db875012dd2"),
            "02720e4bf5...012dd2"
        );
    }

    #[test]
    fn formats_shannons_as_ckb() {
        assert_eq!(format_ckb(100_000_000), "1 CKB");
        assert_eq!(format_ckb(150_000_000), "1.5 CKB");
    }

    #[test]
    fn unknown_selected_node_keeps_inspect_partial_success_shape() {
        let selected = select_nodes(Vec::new(), Some("node-9"));

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].status, InspectStatus::Unreachable);
        assert_eq!(selected[0].rpc_endpoint, "not configured");
    }
}
