//! Writes generated FNN and Docker Compose configuration files.
//! Docker container lifecycle is handled separately by `NetworkManager`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::config::{
    DevkitConfig, NodeConfig, DEFAULT_SECRET_PASSWORD, DOCKER_NETWORK, IMAGE, INTERNAL_P2P_PORT,
    INTERNAL_RPC_PORT,
};
use crate::network::templates;
use crate::AppResult;

/// Writes all generated FNN node files and the Docker Compose reference file.
pub fn generate(project_root: &Path, config: &DevkitConfig) -> AppResult<()> {
    fs::create_dir_all(project_root.join(".fiber/nodes"))?;

    for (index, node) in config.nodes.iter().enumerate() {
        write_node_files(project_root, config, node, index)?;
    }

    write_compose_file(project_root, config)?;
    Ok(())
}

fn write_node_files(
    project_root: &Path,
    config: &DevkitConfig,
    node: &NodeConfig,
    index: usize,
) -> AppResult<()> {
    let node_dir = node.absolute_data_dir(project_root);
    let ckb_dir = node_dir.join("ckb");
    fs::create_dir_all(&ckb_dir)?;

    let config_yaml = render_fnn_config(config, node)?;
    fs::write(node.absolute_config_path(project_root), config_yaml)?;

    // Deterministic keys make local networks reproducible across reset/init cycles.
    let key_path = ckb_dir.join("key");
    let key_hex = deterministic_ckb_key_hex(index);
    fs::write(key_path, format!("{key_hex}\n"))?;

    Ok(())
}

fn deterministic_ckb_key_hex(index: usize) -> String {
    let mut bytes = [0u8; 32];
    bytes[31] = (index as u8).saturating_add(1);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn render_fnn_config(config: &DevkitConfig, node: &NodeConfig) -> AppResult<String> {
    let profile = templates::profile(node.template);
    // RPC listens on the Docker-network DNS name so localhost nodes avoid public-address auth.
    let document = FnnConfigDocument {
        services: vec!["fiber", "rpc", "ckb"],
        fiber: FnnFiberConfig {
            listening_addr: format!("/ip4/0.0.0.0/tcp/{INTERNAL_P2P_PORT}"),
            announced_node_name: node.name.clone(),
            bootnode_addrs: Vec::new(),
            announce_listening_addr: true,
            announce_private_addr: true,
            announced_addrs: vec![format!(
                "/dns4/{}/tcp/{INTERNAL_P2P_PORT}",
                node.container_name
            )],
            chain: "testnet",
            scripts: testnet_scripts(),
            max_inbound_peers: profile.max_inbound_peers,
            min_outbound_peers: profile.min_outbound_peers,
            auto_accept_channel_ckb_funding_amount: profile.auto_accept_channel_ckb_funding_amount,
            open_channel_auto_accept_min_ckb_funding_amount: profile
                .open_channel_auto_accept_min_ckb_funding_amount,
            tlc_fee_proportional_millionths: profile.tlc_fee_proportional_millionths,
            auto_announce_node: true,
            announce_node_interval_seconds: 0,
        },
        rpc: FnnRpcConfig {
            listening_addr: format!("{}:{INTERNAL_RPC_PORT}", node.container_name),
            enabled_modules: vec![
                "cch",
                "channel",
                "graph",
                "info",
                "invoice",
                "payment",
                "peer",
                "watchtower",
            ],
            cors_enabled: true,
        },
        ckb: FnnCkbConfig {
            rpc_url: config.ckb_rpc_url.clone(),
            udt_whitelist: vec![rusd_udt()],
        },
    };

    Ok(serde_yaml::to_string(&document)?)
}

fn write_compose_file(project_root: &Path, config: &DevkitConfig) -> AppResult<()> {
    // BTreeMap keeps generated YAML stable for reviews and scenario fixtures.
    let compose = ComposeFile {
        name: "fiber-devkit",
        services: config
            .nodes
            .iter()
            .map(|node| {
                (
                    node.name.as_str(),
                    ComposeService {
                        image: IMAGE,
                        container_name: node.container_name.as_str(),
                        networks: vec![DOCKER_NETWORK],
                        environment: vec![
                            "FIBER_SECRET_KEY_PASSWORD=fiber-devkit-local",
                            "RUST_LOG=info",
                        ],
                        ports: vec![
                            format!("127.0.0.1:{}:{INTERNAL_RPC_PORT}", node.rpc_port),
                            format!("127.0.0.1:{}:{INTERNAL_P2P_PORT}", node.p2p_port),
                        ],
                        volumes: vec![format!(
                            "{}:/fiber",
                            node.absolute_data_dir(project_root).display()
                        )],
                        command: vec!["fnn", "-c", "/fiber/config.yml", "-d", "/fiber"],
                    },
                )
            })
            .collect(),
        networks: BTreeMap::from([(
            DOCKER_NETWORK,
            ComposeNetwork {
                name: DOCKER_NETWORK,
                driver: "bridge",
            },
        )]),
    };
    let rendered = serde_yaml::to_string(&compose)?;
    fs::write(project_root.join(".fiber/docker-compose.yml"), rendered)?;
    Ok(())
}

#[derive(Serialize)]
struct ComposeFile<'a> {
    name: &'a str,
    services: BTreeMap<&'a str, ComposeService<'a>>,
    networks: BTreeMap<&'a str, ComposeNetwork<'a>>,
}

#[derive(Serialize)]
struct ComposeService<'a> {
    image: &'a str,
    container_name: &'a str,
    networks: Vec<&'a str>,
    environment: Vec<&'a str>,
    ports: Vec<String>,
    volumes: Vec<String>,
    command: Vec<&'a str>,
}

#[derive(Serialize)]
struct ComposeNetwork<'a> {
    name: &'a str,
    driver: &'a str,
}

#[derive(Serialize)]
struct FnnConfigDocument {
    services: Vec<&'static str>,
    fiber: FnnFiberConfig,
    rpc: FnnRpcConfig,
    ckb: FnnCkbConfig,
}

#[derive(Serialize)]
struct FnnFiberConfig {
    listening_addr: String,
    announced_node_name: String,
    bootnode_addrs: Vec<String>,
    announce_listening_addr: bool,
    announce_private_addr: bool,
    announced_addrs: Vec<String>,
    chain: &'static str,
    scripts: Vec<FiberScript>,
    max_inbound_peers: usize,
    min_outbound_peers: usize,
    auto_accept_channel_ckb_funding_amount: u64,
    open_channel_auto_accept_min_ckb_funding_amount: u64,
    tlc_fee_proportional_millionths: u128,
    auto_announce_node: bool,
    announce_node_interval_seconds: u64,
}

#[derive(Serialize)]
struct FnnRpcConfig {
    listening_addr: String,
    enabled_modules: Vec<&'static str>,
    cors_enabled: bool,
}

#[derive(Serialize)]
struct FnnCkbConfig {
    rpc_url: String,
    udt_whitelist: Vec<UdtConfig>,
}

#[derive(Serialize)]
struct FiberScript {
    name: &'static str,
    script: Script,
    cell_deps: Vec<ScriptCellDep>,
}

#[derive(Serialize)]
struct ScriptCellDep {
    #[serde(skip_serializing_if = "Option::is_none")]
    type_id: Option<Script>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cell_dep: Option<CellDep>,
}

#[derive(Serialize)]
struct Script {
    code_hash: &'static str,
    hash_type: &'static str,
    args: &'static str,
}

#[derive(Serialize)]
struct CellDep {
    out_point: OutPoint,
    dep_type: &'static str,
}

#[derive(Serialize)]
struct OutPoint {
    tx_hash: &'static str,
    index: &'static str,
}

#[derive(Serialize)]
struct UdtConfig {
    name: &'static str,
    script: Script,
    cell_deps: Vec<ScriptCellDep>,
    auto_accept_amount: u128,
}

fn testnet_scripts() -> Vec<FiberScript> {
    vec![
        FiberScript {
            name: "FundingLock",
            script: Script {
                code_hash: "0x6c67887fe201ee0c7853f1682c0b77c0e6214044c156c7558269390a8afa6d7c",
                hash_type: "type",
                args: "0x",
            },
            cell_deps: vec![
                ScriptCellDep {
                    type_id: Some(Script {
                        code_hash:
                            "0x00000000000000000000000000000000000000000000000000545950455f4944",
                        hash_type: "type",
                        args: "0x3cb7c0304fe53f75bb5727e2484d0beae4bd99d979813c6fc97c3cca569f10f6",
                    }),
                    cell_dep: None,
                },
                ScriptCellDep {
                    type_id: None,
                    cell_dep: Some(CellDep {
                        out_point: OutPoint {
                            tx_hash:
                                "0x12c569a258dd9c5bd99f632bb8314b1263b90921ba31496467580d6b79dd14a7",
                            index: "0x0",
                        },
                        dep_type: "code",
                    }),
                },
            ],
        },
        FiberScript {
            name: "CommitmentLock",
            script: Script {
                code_hash: "0x740dee83f87c6f309824d8fd3fbdd3c8380ee6fc9acc90b1a748438afcdf81d8",
                hash_type: "type",
                args: "0x",
            },
            cell_deps: vec![
                ScriptCellDep {
                    type_id: Some(Script {
                        code_hash:
                            "0x00000000000000000000000000000000000000000000000000545950455f4944",
                        hash_type: "type",
                        args: "0xf7e458887495cf70dd30d1543cad47dc1dfe9d874177bf19291e4db478d5751b",
                    }),
                    cell_dep: None,
                },
                ScriptCellDep {
                    type_id: None,
                    cell_dep: Some(CellDep {
                        out_point: OutPoint {
                            tx_hash:
                                "0x12c569a258dd9c5bd99f632bb8314b1263b90921ba31496467580d6b79dd14a7",
                            index: "0x0",
                        },
                        dep_type: "code",
                    }),
                },
            ],
        },
    ]
}

fn rusd_udt() -> UdtConfig {
    UdtConfig {
        name: "RUSD",
        script: Script {
            code_hash: "0x1142755a044bf2ee358cba9f2da187ce928c91cd4dc8692ded0337efa677d21a",
            hash_type: "type",
            args: "0x878fcc6f1f08d48e87bb1c3b3d5083f23f8a39c5d5c764f253b55b998526439b",
        },
        cell_deps: vec![ScriptCellDep {
            type_id: Some(Script {
                code_hash: "0x00000000000000000000000000000000000000000000000000545950455f4944",
                hash_type: "type",
                args: "0x97d30b723c0b2c66e9cb8d4d0df4ab5d7222cbb00d4a9a2055ce2e5d7f0d8b0f",
            }),
            cell_dep: None,
        }],
        auto_accept_amount: 1_000_000_000,
    }
}

/// Environment variables shared by every generated FNN container.
pub fn container_env() -> Vec<String> {
    vec![
        format!("FIBER_SECRET_KEY_PASSWORD={DEFAULT_SECRET_PASSWORD}"),
        "RUST_LOG=info".to_string(),
    ]
}

/// Command used to launch FNN inside every generated container.
pub fn container_cmd() -> Vec<String> {
    vec![
        "fnn".to_string(),
        "-c".to_string(),
        "/fiber/config.yml".to_string(),
        "-d".to_string(),
        "/fiber".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{default_node_configs, DEFAULT_CKB_RPC_URL};

    #[test]
    fn deterministic_key_is_32_byte_hex() {
        let key = deterministic_ckb_key_hex(0);
        assert_eq!(key.len(), 64);
        assert!(key.ends_with("01"));
    }

    #[test]
    fn fnn_config_uses_container_ports_and_docker_dns() {
        let config = DevkitConfig {
            version: 1,
            image: IMAGE.to_string(),
            ckb_rpc_url: DEFAULT_CKB_RPC_URL.to_string(),
            topology: crate::config::Topology::HubSpoke,
            nodes: default_node_configs(crate::config::Topology::HubSpoke, 1),
        };
        let rendered = render_fnn_config(&config, &config.nodes[0]).unwrap();

        assert!(rendered.contains("fiber-devkit-node-1:8227"));
        assert!(rendered.contains("/ip4/0.0.0.0/tcp/8228"));
        assert!(rendered.contains("/dns4/fiber-devkit-node-1/tcp/8228"));
    }
}
