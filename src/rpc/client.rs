//! Thin HTTP JSON-RPC 2.0 client for FNN.
//! Constructs request envelopes and decodes responses for the current DevKit demos.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::Value;

use crate::rpc::types::{
    Channel, GraphChannel, GraphNode, JsonRpcError, NodeInfo, OpenChannelParams, OpenChannelResult,
    Payment, RpcError, RpcResult, SendPaymentParams,
};

/// JSON-RPC client bound to one Fiber node endpoint.
pub struct FiberRpc {
    endpoint: String,
    client: reqwest::Client,
    next_id: AtomicU64,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    result: Option<Value>,
    error: Option<JsonRpcError>,
}

impl FiberRpc {
    /// Creates an RPC client with a short timeout for scenario steps.
    pub fn new(endpoint: impl Into<String>) -> RpcResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|err| RpcError::Http {
                message: err.to_string(),
            })?;

        Ok(Self {
            endpoint: endpoint.into(),
            client,
            next_id: AtomicU64::new(1),
        })
    }

    /// Calls FNN `node_info`.
    pub async fn node_info(&self) -> RpcResult<NodeInfo> {
        let value = self.call("node_info", Vec::new()).await?;
        NodeInfo::from_value(value)
    }

    /// Calls FNN `list_channels`.
    pub async fn list_channels(&self) -> RpcResult<Vec<Channel>> {
        self.list_channels_with_options(false, false).await
    }

    /// Calls FNN `list_channels` including non-ready opening states.
    pub async fn list_pending_channels(&self) -> RpcResult<Vec<Channel>> {
        self.list_channels_with_options(false, true).await
    }

    /// Calls FNN `list_channels` including closed or failed channels.
    pub async fn list_all_channels(&self) -> RpcResult<Vec<Channel>> {
        self.list_channels_with_options(true, false).await
    }

    async fn list_channels_with_options(
        &self,
        include_closed: bool,
        only_pending: bool,
    ) -> RpcResult<Vec<Channel>> {
        let value = self
            .call(
                "list_channels",
                vec![serde_json::json!({
                    "include_closed": include_closed,
                    "only_pending": only_pending
                })],
            )
            .await?;

        let channels = value
            .get("channels")
            .and_then(Value::as_array)
            .or_else(|| value.as_array())
            .ok_or_else(|| RpcError::InvalidResponse {
                message: "list_channels response did not include channels".to_string(),
                raw: Some(value.clone()),
            })?;

        Ok(channels.iter().cloned().map(Channel::from_value).collect())
    }

    /// Calls FNN `open_channel`.
    pub async fn open_channel(&self, params: OpenChannelParams) -> RpcResult<OpenChannelResult> {
        let value = self
            .call("open_channel", vec![params.to_rpc_value()])
            .await?;
        Ok(OpenChannelResult::from_value(value))
    }

    /// Calls FNN `send_payment`.
    pub async fn send_payment(&self, params: SendPaymentParams) -> RpcResult<Payment> {
        let value = self
            .call("send_payment", vec![params.to_rpc_value()])
            .await?;
        Ok(Payment::from_value(value))
    }

    /// Calls FNN `get_payment`.
    pub async fn get_payment(&self, payment_hash: &str) -> RpcResult<Payment> {
        let value = self
            .call(
                "get_payment",
                vec![serde_json::json!({
                    "payment_hash": payment_hash
                })],
            )
            .await?;
        Ok(Payment::from_value(value))
    }

    /// Calls FNN `graph_nodes`.
    pub async fn graph_nodes(&self) -> RpcResult<Vec<GraphNode>> {
        let value = self
            .call("graph_nodes", vec![serde_json::json!({})])
            .await?;
        let nodes = value
            .get("nodes")
            .and_then(Value::as_array)
            .or_else(|| value.as_array())
            .ok_or_else(|| RpcError::InvalidResponse {
                message: "graph_nodes response did not include nodes".to_string(),
                raw: Some(value.clone()),
            })?;

        Ok(nodes.iter().cloned().map(GraphNode::from_value).collect())
    }

    /// Calls FNN `graph_channels`.
    pub async fn graph_channels(&self) -> RpcResult<Vec<GraphChannel>> {
        let value = self
            .call("graph_channels", vec![serde_json::json!({})])
            .await?;
        let channels = value
            .get("channels")
            .and_then(Value::as_array)
            .or_else(|| value.as_array())
            .ok_or_else(|| RpcError::InvalidResponse {
                message: "graph_channels response did not include channels".to_string(),
                raw: Some(value.clone()),
            })?;

        Ok(channels
            .iter()
            .cloned()
            .map(GraphChannel::from_value)
            .collect())
    }

    async fn call(&self, method: &str, params: Vec<Value>) -> RpcResult<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        // FNN expects JSON-RPC params to be an array; object params are wrapped as one item.
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": id
        });

        let response = self
            .client
            .post(&self.endpoint)
            .json(&request)
            .send()
            .await
            .map_err(|err| RpcError::Http {
                message: err.to_string(),
            })?;

        let status = response.status();
        if status != StatusCode::OK {
            return Err(RpcError::Http {
                message: format!("{} returned HTTP {}", self.endpoint, status),
            });
        }

        let raw = response
            .json::<JsonRpcResponse>()
            .await
            .map_err(|err| RpcError::Json {
                message: err.to_string(),
            })?;

        if let Some(error) = raw.error {
            return Err(RpcError::Rpc {
                code: error.code,
                message: error.message,
                data: error.data,
            });
        }

        raw.result.ok_or_else(|| RpcError::InvalidResponse {
            message: "JSON-RPC response omitted both result and error".to_string(),
            raw: None,
        })
    }
}
