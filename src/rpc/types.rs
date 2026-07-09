//! RPC data types for Fiber nodes.
//! Defines narrow request and response wrappers for scenarios and route prediction.

use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Result type returned by the Fiber RPC client.
pub type RpcResult<T> = Result<T, RpcError>;

/// Error returned by HTTP transport, JSON decoding, or JSON-RPC failure bodies.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RpcError {
    Http {
        message: String,
    },
    Json {
        message: String,
    },
    Rpc {
        code: i64,
        message: String,
        data: Option<Value>,
    },
    InvalidResponse {
        message: String,
        raw: Option<Value>,
    },
}

impl RpcError {
    /// Returns a compact message suitable for step-level structured output.
    pub fn message(&self) -> String {
        match self {
            Self::Http { message }
            | Self::Json { message }
            | Self::InvalidResponse { message, .. } => message.clone(),
            Self::Rpc { message, .. } => message.clone(),
        }
    }
}

impl fmt::Display for RpcError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http { message } => write!(formatter, "HTTP error: {message}"),
            Self::Json { message } => write!(formatter, "JSON error: {message}"),
            Self::Rpc { code, message, .. } => write!(formatter, "RPC error {code}: {message}"),
            Self::InvalidResponse { message, .. } => {
                write!(formatter, "invalid RPC response: {message}")
            }
        }
    }
}

impl Error for RpcError {}

/// JSON-RPC error object returned by FNN.
#[derive(Debug, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

/// Minimal node identity fields consumed by scenarios and route prediction.
#[derive(Debug, Clone, Serialize)]
pub struct NodeInfo {
    pub pubkey: String,
    pub node_name: Option<String>,
    pub addresses: Vec<String>,
    pub channel_count: Option<u64>,
    pub peers_count: Option<u64>,
    pub raw: Value,
}

impl NodeInfo {
    /// Extracts stable fields from the larger `node_info` RPC response.
    pub fn from_value(raw: Value) -> RpcResult<Self> {
        let pubkey = raw
            .get("pubkey")
            .and_then(Value::as_str)
            .ok_or_else(|| RpcError::InvalidResponse {
                message: "node_info response did not include pubkey".to_string(),
                raw: Some(raw.clone()),
            })?
            .to_string();

        let node_name = raw
            .get("node_name")
            .and_then(Value::as_str)
            .map(str::to_string);
        let addresses = raw
            .get("addresses")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();

        Ok(Self {
            pubkey,
            node_name,
            addresses,
            channel_count: raw.get("channel_count").and_then(value_to_u64),
            peers_count: raw.get("peers_count").and_then(value_to_u64),
            raw,
        })
    }
}

/// Minimal channel fields consumed by setup and readiness checks.
#[derive(Debug, Clone, Serialize)]
pub struct Channel {
    pub channel_id: Option<String>,
    pub pubkey: Option<String>,
    pub state_name: Option<String>,
    pub local_balance: Option<u128>,
    pub remote_balance: Option<u128>,
    pub enabled: Option<bool>,
    pub raw: Value,
}

impl Channel {
    /// Extracts channel fields while preserving the raw response for diagnostics.
    pub fn from_value(raw: Value) -> Self {
        Self {
            channel_id: raw
                .get("channel_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            pubkey: raw
                .get("pubkey")
                .and_then(Value::as_str)
                .map(str::to_string),
            state_name: raw
                .get("state")
                .and_then(channel_state_name)
                .map(str::to_string),
            local_balance: raw.get("local_balance").and_then(value_to_u128),
            remote_balance: raw.get("remote_balance").and_then(value_to_u128),
            enabled: raw.get("enabled").and_then(Value::as_bool),
            raw,
        }
    }

    /// A channel is usable for payments only after FNN reports `ChannelReady`.
    pub fn is_ready_with(&self, pubkey: &str) -> bool {
        self.pubkey.as_deref() == Some(pubkey)
            && self.state_name.as_deref() == Some("ChannelReady")
            && self.enabled.unwrap_or(true)
    }

    /// Returns true when FNN has stopped trying to open this channel.
    pub fn is_closed_with(&self, pubkey: &str) -> bool {
        self.pubkey.as_deref() == Some(pubkey) && self.state_name.as_deref() == Some("Closed")
    }
}

/// Parameters for the `open_channel` RPC call.
#[derive(Debug, Clone)]
pub struct OpenChannelParams {
    pub pubkey: String,
    pub funding_amount: u128,
    pub public: bool,
    pub one_way: bool,
}

impl OpenChannelParams {
    /// Builds the exact JSON object shape expected by FNN.
    pub fn to_rpc_value(&self) -> Value {
        serde_json::json!({
            "pubkey": self.pubkey,
            "funding_amount": u128_hex(self.funding_amount),
            "public": self.public,
            "one_way": self.one_way
        })
    }
}

/// Minimal result returned from `open_channel`.
#[derive(Debug, Clone, Serialize)]
pub struct OpenChannelResult {
    pub temporary_channel_id: Option<String>,
    pub raw: Value,
}

impl OpenChannelResult {
    /// Extracts the temporary channel ID while retaining the original payload.
    pub fn from_value(raw: Value) -> Self {
        Self {
            temporary_channel_id: raw
                .get("temporary_channel_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            raw,
        }
    }
}

/// Parameters for the `send_payment` RPC call.
#[derive(Debug, Clone)]
pub struct SendPaymentParams {
    pub target_pubkey: String,
    pub amount: u128,
    pub timeout_seconds: u64,
    pub max_fee_amount: Option<u128>,
    pub dry_run: bool,
}

impl SendPaymentParams {
    /// Builds a keysend payment request without invoices or custom routing.
    pub fn to_rpc_value(&self) -> Value {
        let mut value = serde_json::json!({
            "target_pubkey": self.target_pubkey,
            "amount": u128_hex(self.amount),
            "timeout": u64_hex(self.timeout_seconds),
            "keysend": true,
            "dry_run": self.dry_run
        });

        if let Some(max_fee_amount) = self.max_fee_amount {
            value["max_fee_amount"] = Value::String(u128_hex(max_fee_amount));
        }

        value
    }
}

/// Minimal payment status returned by `send_payment` and `get_payment`.
#[derive(Debug, Clone, Serialize)]
pub struct Payment {
    pub payment_hash: Option<String>,
    pub status: Option<String>,
    pub failed_error: Option<String>,
    pub raw: Value,
}

impl Payment {
    /// Extracts stable status fields while preserving the raw payment object.
    pub fn from_value(raw: Value) -> Self {
        Self {
            payment_hash: raw
                .get("payment_hash")
                .and_then(Value::as_str)
                .map(str::to_string),
            status: raw
                .get("status")
                .and_then(Value::as_str)
                .map(str::to_string),
            failed_error: raw
                .get("failed_error")
                .and_then(Value::as_str)
                .map(str::to_string),
            raw,
        }
    }

    /// Returns true when FNN has settled the payment successfully.
    pub fn is_success(&self) -> bool {
        self.status.as_deref() == Some("Success")
    }

    /// Returns true when FNN has reported a terminal failed payment.
    pub fn is_failed(&self) -> bool {
        self.status.as_deref() == Some("Failed") || self.failed_error.is_some()
    }
}

/// Minimal graph node wrapper.
#[derive(Debug, Clone, Serialize)]
pub struct GraphNode {
    pub pubkey: Option<String>,
    pub raw: Value,
}

impl GraphNode {
    /// Extracts a graph node from the larger graph response object.
    pub fn from_value(raw: Value) -> Self {
        Self {
            pubkey: raw
                .get("pubkey")
                .and_then(Value::as_str)
                .map(str::to_string),
            raw,
        }
    }
}

/// Minimal graph channel wrapper.
#[derive(Debug, Clone, Serialize)]
pub struct GraphChannel {
    pub channel_outpoint: Option<Value>,
    pub node1: Option<String>,
    pub node2: Option<String>,
    pub capacity: Option<u128>,
    pub raw: Value,
}

impl GraphChannel {
    /// Extracts a graph channel from the larger graph response object.
    pub fn from_value(raw: Value) -> Self {
        Self {
            channel_outpoint: raw.get("channel_outpoint").cloned(),
            node1: raw.get("node1").and_then(Value::as_str).map(str::to_string),
            node2: raw.get("node2").and_then(Value::as_str).map(str::to_string),
            capacity: raw.get("capacity").and_then(value_to_u128),
            raw,
        }
    }
}

/// Converts shannons to FNN's hex quantity string.
pub fn u128_hex(value: u128) -> String {
    format!("0x{value:x}")
}

/// Converts an integer to FNN's hex quantity string.
pub fn u64_hex(value: u64) -> String {
    format!("0x{value:x}")
}

/// Parses either FNN hex quantities or plain JSON numbers.
pub fn value_to_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(parse_quantity_u64))
}

/// Parses either FNN hex quantities or plain JSON numbers.
pub fn value_to_u128(value: &Value) -> Option<u128> {
    value
        .as_u64()
        .map(u128::from)
        .or_else(|| value.as_str().and_then(parse_quantity_u128))
}

fn parse_quantity_u64(text: &str) -> Option<u64> {
    if let Some(hex) = text.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).ok()
    } else {
        text.parse().ok()
    }
}

fn parse_quantity_u128(text: &str) -> Option<u128> {
    if let Some(hex) = text.strip_prefix("0x") {
        u128::from_str_radix(hex, 16).ok()
    } else {
        text.parse().ok()
    }
}

fn channel_state_name(value: &Value) -> Option<&str> {
    value
        .as_str()
        .or_else(|| value.get("state_name").and_then(Value::as_str))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_amounts_as_hex_quantities() {
        assert_eq!(u128_hex(100_000_000), "0x5f5e100");
        assert_eq!(u64_hex(20), "0x14");
    }

    #[test]
    fn parses_channel_ready_state_from_adjacent_tag() {
        let channel = Channel::from_value(serde_json::json!({
            "pubkey": "abc",
            "state": { "state_name": "ChannelReady", "state_flags": "0x0" },
            "enabled": true
        }));

        assert!(channel.is_ready_with("abc"));
    }
}
