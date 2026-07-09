//! Docker-backed local Fiber network manager.
//! Owns init/up/down/reset behavior and container cleanup; it deliberately does
//! not implement scenario execution or typed Fiber RPC calls.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use bollard::container::{
    Config as ContainerConfig, CreateContainerOptions, InspectContainerOptions, NetworkingConfig,
    RemoveContainerOptions, StartContainerOptions, StopContainerOptions, WaitContainerOptions,
};
use bollard::image::CreateImageOptions;
use bollard::models::{EndpointSettings, HostConfig, PortBinding};
use bollard::network::{CreateNetworkOptions, InspectNetworkOptions};
use bollard::Docker;
use futures_util::TryStreamExt;
use serde_json::Value;
use tokio::time::sleep;

use crate::config::{
    default_node_configs, DevkitConfig, NodeConfig, Topology, DEFAULT_CKB_RPC_URL, DOCKER_NETWORK,
    FIBER_DIR, IMAGE, IMAGE_REPOSITORY, IMAGE_TAG, INTERNAL_P2P_PORT, INTERNAL_RPC_PORT,
};
use crate::network::compose;
use crate::{app_error, AppResult};

/// Options used when generating a new local network config.
pub struct InitOptions {
    pub nodes: usize,
    pub topology: Topology,
}

/// Coordinates `.fiber/` config generation and Docker container lifecycle.
pub struct NetworkManager {
    project_root: PathBuf,
}

impl NetworkManager {
    /// Creates a manager bound to a project root.
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }

    /// Generates `.fiber/config.toml`, node FNN config, and Docker metadata.
    pub fn init(&self, options: InitOptions) -> AppResult<DevkitConfig> {
        if options.nodes == 0 {
            return Err(app_error("`--nodes` must be at least 1"));
        }

        fs::create_dir_all(self.project_root.join(FIBER_DIR))?;

        let config = DevkitConfig {
            version: 1,
            image: IMAGE.to_string(),
            ckb_rpc_url: DEFAULT_CKB_RPC_URL.to_string(),
            topology: options.topology,
            nodes: default_node_configs(options.topology, options.nodes),
        };

        // Render node files before the project config so partial init failures are obvious.
        compose::generate(&self.project_root, &config)?;
        config.write_to_project(&self.project_root)?;

        Ok(config)
    }

    /// Starts all configured containers, waits for JSON-RPC, then connects hub-spoke peers.
    pub async fn up(&self) -> AppResult<()> {
        let config = DevkitConfig::read_from_project(&self.project_root)?;
        let docker = Docker::connect_with_local_defaults()?;
        docker.ping().await?;

        self.ensure_image(&docker).await?;
        self.ensure_network(&docker).await?;

        for node in &config.nodes {
            self.recreate_container(&docker, &config, node).await?;
            let rpc_endpoint = self
                .get_node_rpc(&config, &node.name)
                .unwrap_or_else(|| node.rpc_endpoint());
            println!(
                "Started {} ({}) on RPC {}",
                node.name, node.container_name, rpc_endpoint
            );
        }

        let infos = self.wait_for_nodes(&docker, &config).await?;
        self.connect_hub_spoke(&config, &infos).await?;

        println!(
            "Fiber network is up: {} nodes reachable",
            config.nodes.len()
        );
        Ok(())
    }

    /// Stops and removes all configured containers without deleting `.fiber/`.
    pub async fn down(&self) -> AppResult<()> {
        let config = match DevkitConfig::read_from_project(&self.project_root) {
            Ok(config) => config,
            Err(_) => {
                println!("No .fiber/config.toml found; nothing to stop");
                return Ok(());
            }
        };

        let docker = Docker::connect_with_local_defaults()?;
        docker.ping().await?;

        for node in &config.nodes {
            self.stop_and_remove_container(&docker, &node.container_name)
                .await?;
        }

        println!("Fiber network is down");
        Ok(())
    }

    /// Removes generated state and reinitializes using the previous topology when available.
    pub async fn reset(&self) -> AppResult<()> {
        let previous = DevkitConfig::read_from_project(&self.project_root).ok();
        self.down().await?;

        let fiber_dir = self.project_root.join(FIBER_DIR);
        if fiber_dir.exists() {
            make_tree_writable(&fiber_dir)?;
            if let Err(err) = fs::remove_dir_all(&fiber_dir) {
                println!(
                    "Host cleanup could not remove .fiber ({err}); retrying with a one-shot Docker cleanup container"
                );
                let docker = Docker::connect_with_local_defaults()?;
                docker.ping().await?;
                self.cleanup_fiber_dir_with_docker(&docker).await?;
            }
        }

        // Preserve the user's node count/topology across reset, falling back to Demo 1 defaults.
        let options = previous
            .map(|config| InitOptions {
                nodes: config.nodes.len(),
                topology: config.topology,
            })
            .unwrap_or(InitOptions {
                nodes: 3,
                topology: Topology::HubSpoke,
            });

        self.init(options)?;
        println!("Fiber network reset complete");
        Ok(())
    }

    /// Returns the localhost JSON-RPC endpoint for a configured node name.
    pub fn get_node_rpc(&self, config: &DevkitConfig, name: &str) -> Option<String> {
        config
            .nodes
            .iter()
            .find(|node| node.name == name)
            .map(NodeConfig::rpc_endpoint)
    }

    async fn ensure_image(&self, docker: &Docker) -> AppResult<()> {
        if docker.inspect_image(IMAGE).await.is_ok() {
            println!("Using cached image {IMAGE}");
            return Ok(());
        }

        println!("Pulling pinned FNN image {IMAGE}");
        pull_image(docker, IMAGE_REPOSITORY, IMAGE_TAG).await?;
        docker.inspect_image(IMAGE).await?;
        Ok(())
    }

    async fn ensure_network(&self, docker: &Docker) -> AppResult<()> {
        if docker
            .inspect_network(DOCKER_NETWORK, None::<InspectNetworkOptions<String>>)
            .await
            .is_ok()
        {
            return Ok(());
        }

        let mut labels = HashMap::new();
        labels.insert("dev.fiber-devkit.managed".to_string(), "true".to_string());
        labels.insert(
            "dev.fiber-devkit.project".to_string(),
            self.project_root.display().to_string(),
        );

        docker
            .create_network(CreateNetworkOptions {
                name: DOCKER_NETWORK.to_string(),
                check_duplicate: true,
                driver: "bridge".to_string(),
                labels,
                ..Default::default()
            })
            .await?;

        Ok(())
    }

    async fn recreate_container(
        &self,
        docker: &Docker,
        config: &DevkitConfig,
        node: &NodeConfig,
    ) -> AppResult<()> {
        self.stop_and_remove_container(docker, &node.container_name)
            .await?;

        let mut labels = HashMap::new();
        labels.insert("dev.fiber-devkit.managed".to_string(), "true".to_string());
        labels.insert(
            "dev.fiber-devkit.project".to_string(),
            self.project_root.display().to_string(),
        );
        labels.insert("dev.fiber-devkit.node".to_string(), node.name.clone());

        let host_config = HostConfig {
            binds: Some(vec![format!(
                "{}:/fiber",
                node.absolute_data_dir(&self.project_root).display()
            )]),
            port_bindings: Some(port_bindings(node)),
            ..Default::default()
        };

        let networking_config = NetworkingConfig {
            endpoints_config: HashMap::from([(
                DOCKER_NETWORK.to_string(),
                EndpointSettings {
                    aliases: Some(vec![node.container_name.clone(), node.name.clone()]),
                    ..Default::default()
                },
            )]),
        };

        let container_config = ContainerConfig {
            image: Some(config.image.clone()),
            env: Some(compose::container_env()),
            cmd: Some(compose::container_cmd()),
            host_config: Some(host_config),
            networking_config: Some(networking_config),
            labels: Some(labels),
            ..Default::default()
        };

        docker
            .create_container(
                Some(CreateContainerOptions {
                    name: node.container_name.clone(),
                    platform: None,
                }),
                container_config,
            )
            .await?;

        docker
            .start_container(&node.container_name, None::<StartContainerOptions<String>>)
            .await?;

        Ok(())
    }

    async fn stop_and_remove_container(&self, docker: &Docker, name: &str) -> AppResult<()> {
        if docker
            .inspect_container(name, None::<InspectContainerOptions>)
            .await
            .is_err()
        {
            return Ok(());
        }

        // Stop can fail when the container already exited; removal is the authoritative cleanup.
        let _ = docker
            .stop_container(name, Some(StopContainerOptions { t: 10 }))
            .await;

        docker
            .remove_container(
                name,
                Some(RemoveContainerOptions {
                    force: true,
                    v: true,
                    ..Default::default()
                }),
            )
            .await?;

        println!("Removed container {name}");
        Ok(())
    }

    async fn cleanup_fiber_dir_with_docker(&self, docker: &Docker) -> AppResult<()> {
        let name = "fiber-devkit-cleanup";
        self.stop_and_remove_container(docker, name).await?;

        // FNN may create Docker-owned files under `.fiber`; the cleanup container removes them
        // from the same mount namespace without requiring host sudo.
        let host_config = HostConfig {
            binds: Some(vec![format!("{}:/work", self.project_root.display())]),
            ..Default::default()
        };
        let container_config = ContainerConfig {
            image: Some(IMAGE.to_string()),
            cmd: Some(vec![
                "sh".to_string(),
                "-c".to_string(),
                "chmod -R u+rwx /work/.fiber 2>/dev/null || true; rm -rf /work/.fiber".to_string(),
            ]),
            host_config: Some(host_config),
            ..Default::default()
        };

        docker
            .create_container(
                Some(CreateContainerOptions {
                    name: name.to_string(),
                    platform: None,
                }),
                container_config,
            )
            .await?;
        docker
            .start_container(name, None::<StartContainerOptions<String>>)
            .await?;

        let mut wait = docker.wait_container(name, None::<WaitContainerOptions<String>>);
        while let Some(result) = wait.try_next().await? {
            if result.status_code != 0 {
                return Err(app_error(format!(
                    "Docker cleanup container exited with status {}",
                    result.status_code
                )));
            }
        }

        docker
            .remove_container(
                name,
                Some(RemoveContainerOptions {
                    force: true,
                    v: true,
                    ..Default::default()
                }),
            )
            .await?;

        Ok(())
    }

    async fn wait_for_nodes(
        &self,
        docker: &Docker,
        config: &DevkitConfig,
    ) -> AppResult<HashMap<String, Value>> {
        let mut infos = HashMap::new();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()?;

        for node in &config.nodes {
            let info = self.wait_for_node_info(docker, &client, node).await?;
            infos.insert(node.name.clone(), info);
        }

        Ok(infos)
    }

    async fn wait_for_node_info(
        &self,
        docker: &Docker,
        client: &reqwest::Client,
        node: &NodeConfig,
    ) -> AppResult<Value> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "node_info",
            "params": [],
            "id": 1
        });

        let endpoint = node.rpc_endpoint();
        let mut last_error = String::new();
        // Startup is asynchronous: Docker may report running before FNN RPC is ready.
        for _ in 0..60 {
            match client.post(&endpoint).json(&body).send().await {
                Ok(response) => match response.json::<Value>().await {
                    Ok(payload) if payload.get("result").is_some() => {
                        println!("{} answered node_info", node.name);
                        return Ok(payload["result"].clone());
                    }
                    Ok(payload) => {
                        last_error = format!("unexpected RPC payload: {payload}");
                    }
                    Err(err) => {
                        last_error = format!("invalid RPC response: {err}");
                    }
                },
                Err(err) => {
                    last_error = err.to_string();
                }
            }

            if let Ok(inspect) = docker
                .inspect_container(&node.container_name, None::<InspectContainerOptions>)
                .await
            {
                if inspect
                    .state
                    .and_then(|state| state.running)
                    .is_some_and(|running| !running)
                {
                    return Err(app_error(format!(
                        "{} exited before RPC became ready. Last RPC error: {}",
                        node.container_name, last_error
                    )));
                }
            }

            sleep(Duration::from_secs(1)).await;
        }

        Err(app_error(format!(
            "{} did not answer node_info at {} within 60s. Last error: {}",
            node.name, endpoint, last_error
        )))
    }

    async fn connect_hub_spoke(
        &self,
        config: &DevkitConfig,
        infos: &HashMap<String, Value>,
    ) -> AppResult<()> {
        if !matches!(config.topology, Topology::HubSpoke) || config.nodes.len() <= 1 {
            return Ok(());
        }

        let hub = &config.nodes[0];
        let hub_info = infos
            .get(&hub.name)
            .ok_or_else(|| app_error("hub node_info missing after startup"))?;
        // Prefer the Docker DNS multiaddr; `0.0.0.0` is valid for listening but not dialing.
        let hub_address = hub_info
            .get("addresses")
            .and_then(Value::as_array)
            .and_then(|addresses| {
                addresses
                    .iter()
                    .filter_map(Value::as_str)
                    .find(|address| address.contains(&format!("/dns4/{}/", hub.container_name)))
                    .or_else(|| {
                        addresses
                            .iter()
                            .filter_map(Value::as_str)
                            .find(|address| !address.contains("/ip4/0.0.0.0/"))
                    })
            })
            .map(str::to_string)
            .unwrap_or_else(|| format!("/dns4/{}/tcp/{INTERNAL_P2P_PORT}", hub.container_name));

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;

        for leaf in config.nodes.iter().skip(1) {
            let body = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "connect_peer",
                "params": [{
                    "address": hub_address,
                    "save": true
                }],
                "id": 1
            });

            match client.post(leaf.rpc_endpoint()).json(&body).send().await {
                Ok(response) => {
                    let payload = response.json::<Value>().await.unwrap_or(Value::Null);
                    if payload.get("error").is_some() {
                        println!(
                            "Warning: {} could not connect to hub {}: {}",
                            leaf.name, hub.name, payload
                        );
                    } else {
                        println!("Connected {} to hub {}", leaf.name, hub.name);
                    }
                }
                Err(err) => {
                    println!(
                        "Warning: {} could not connect to hub {}: {}",
                        leaf.name, hub.name, err
                    );
                }
            }
        }

        Ok(())
    }
}

fn make_tree_writable(path: &std::path::Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    let mut permissions = metadata.permissions();
    if permissions.readonly() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if metadata.is_dir() { 0o700 } else { 0o600 };
            permissions.set_mode(mode);
        }
        #[cfg(not(unix))]
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)?;
    }

    if metadata.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            return Ok(());
        };
        for entry in entries.flatten() {
            let _ = make_tree_writable(&entry.path());
        }
    }

    Ok(())
}

fn port_bindings(node: &NodeConfig) -> HashMap<String, Option<Vec<PortBinding>>> {
    HashMap::from([
        (
            format!("{INTERNAL_RPC_PORT}/tcp"),
            Some(vec![PortBinding {
                host_ip: Some("127.0.0.1".to_string()),
                host_port: Some(node.rpc_port.to_string()),
            }]),
        ),
        (
            format!("{INTERNAL_P2P_PORT}/tcp"),
            Some(vec![PortBinding {
                host_ip: Some("127.0.0.1".to_string()),
                host_port: Some(node.p2p_port.to_string()),
            }]),
        ),
    ])
}

async fn pull_image(docker: &Docker, repository: &str, tag: &str) -> AppResult<()> {
    let options = Some(CreateImageOptions {
        from_image: repository,
        tag,
        ..Default::default()
    });

    let mut stream = docker.create_image(options, None, None);
    while let Some(_item) = stream.try_next().await? {}
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_project() -> PathBuf {
        let unique = format!(
            "fiber-devkit-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir().join(unique)
    }

    #[test]
    fn init_writes_config_and_node_files() {
        let root = temp_project();
        fs::create_dir_all(&root).unwrap();
        let manager = NetworkManager::new(root.clone());

        let config = manager
            .init(InitOptions {
                nodes: 3,
                topology: Topology::HubSpoke,
            })
            .unwrap();

        assert_eq!(config.nodes.len(), 3);
        assert!(DevkitConfig::path(&root).exists());
        assert!(root.join(".fiber/docker-compose.yml").exists());
        assert!(root.join(&config.nodes[0].config_path).exists());
        assert_eq!(
            manager.get_node_rpc(&config, "node-1").unwrap(),
            "http://127.0.0.1:8227/"
        );

        fs::remove_dir_all(root).unwrap();
    }
}
