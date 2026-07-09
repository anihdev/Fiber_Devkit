//! Native Fiber route prediction engine.
//! Collects graph and channel data and applies heuristic route scoring.

use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use tokio::time::sleep;

use crate::config::DevkitConfig;
use crate::diagnostic::taxonomy::entry_by_code;
use crate::route::cch::{CchPathResult, RouteChoice, RouteComparison};
use crate::rpc::client::FiberRpc;
use crate::rpc::types::{Channel, GraphChannel, NodeInfo, RpcError};
use crate::scenario::parser::parse_ckb_amount;
use crate::{app_error, AppResult};

const DEFAULT_FEE_TOLERANCE_SHANNONS: u128 = 1_000_000;

/// Structured native Fiber prediction returned by `fiber predict`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentPrediction {
    pub from: String,
    pub to: String,
    pub amount: String,
    pub asset: String,
    pub probability: f64,
    pub estimated_fee: String,
    pub estimated_latency: String,
    pub hop_count: usize,
    pub confidence: Confidence,
    pub warnings: Vec<PredictionWarning>,
    pub best_route: Option<RankedPath>,
    pub alternative_routes: Vec<RankedPath>,
}

/// Human-readable confidence bucket derived from the heuristic probability.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

/// Diagnostic warning attached to a prediction.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PredictionWarning {
    pub code: String,
    pub category: String,
    pub message: String,
}

/// Source of route data used to score a candidate route.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteDataSource {
    LocalBalances,
    GraphTopology,
}

/// One candidate route scored by the analyzer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedPath {
    pub path: Vec<String>,
    pub probability: f64,
    pub hop_count: usize,
    pub capacity_score: f64,
    pub hop_penalty: f64,
    pub channel_health: f64,
    pub fee_cost: f64,
    pub min_outbound_capacity: String,
    pub estimated_fee: String,
    pub data_source: RouteDataSource,
}

/// Analyzer bound to one project root and its `.fiber/config.toml`.
pub struct RouteAnalyzer {
    project_root: PathBuf,
}

/// Scenario alias to generated node name used when a scenario invokes prediction.
pub type NodeAliasMap = HashMap<String, String>;

#[derive(Debug, Clone)]
struct NodeSnapshot {
    alias: String,
    configured_name: String,
    endpoint: String,
    pubkey: String,
}

#[derive(Debug, Clone)]
struct RouteEdge {
    from: String,
    to: String,
    outbound_capacity: u128,
    enabled: bool,
    source: RouteDataSource,
}

#[derive(Debug, Clone)]
struct CandidatePath {
    nodes: Vec<String>,
    edges: Vec<RouteEdge>,
}

impl RouteAnalyzer {
    /// Creates an analyzer bound to a project root.
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }

    /// Predicts whether a native Fiber payment can route without sending it.
    pub async fn can_pay(
        &self,
        from: &str,
        to: &str,
        amount: &str,
        asset: &str,
    ) -> AppResult<PaymentPrediction> {
        self.can_pay_with_aliases(from, to, amount, asset, None)
            .await
    }

    /// Predicts route confidence using caller-provided aliases for output labels.
    pub async fn can_pay_with_aliases(
        &self,
        from: &str,
        to: &str,
        amount: &str,
        asset: &str,
        aliases: Option<&NodeAliasMap>,
    ) -> AppResult<PaymentPrediction> {
        let normalized_amount = normalize_amount(amount);
        let amount_shannons = parse_ckb_amount(&normalized_amount)?;
        if !asset.eq_ignore_ascii_case("CKB") {
            return Ok(unsupported_native_asset_prediction(
                from,
                to,
                &normalized_amount,
                asset,
            ));
        }

        let config = DevkitConfig::read_from_project(&self.project_root)?;
        let snapshots = self.collect_nodes(&config, aliases).await?;
        let from_snapshot = snapshot_by_alias(&snapshots, from)?;
        let to_snapshot = snapshot_by_alias(&snapshots, to)?;
        let edges = self.collect_edges(&snapshots).await?;
        let mut ranked = self.find_paths(from_snapshot, to_snapshot, amount_shannons, &edges);
        ranked.sort_by(|left, right| {
            right
                .probability
                .partial_cmp(&left.probability)
                .unwrap_or(Ordering::Equal)
        });
        relabel_paths(&mut ranked, &snapshots);
        dedupe_ranked_paths(&mut ranked);

        let best_route = ranked.first().cloned();
        let probability = best_route
            .as_ref()
            .map(|route| route.probability)
            .unwrap_or(0.0);
        let warnings = prediction_warnings(&ranked, amount_shannons, from, to);
        let estimated_fee = best_route
            .as_ref()
            .map(|route| route.estimated_fee.clone())
            .unwrap_or_else(|| format_ckb(0));
        let hop_count = best_route
            .as_ref()
            .map(|route| route.hop_count)
            .unwrap_or(0);
        let alternative_routes = ranked.iter().skip(1).take(3).cloned().collect();

        let mut prediction = PaymentPrediction {
            from: from.to_string(),
            to: to.to_string(),
            amount: normalized_amount,
            asset: asset.to_string(),
            probability,
            estimated_fee,
            estimated_latency: estimated_latency(hop_count),
            hop_count,
            confidence: confidence(probability),
            warnings,
            best_route,
            alternative_routes,
        };

        if prediction.probability < 0.85 {
            if let Some(alternative) = self.suggest_alternative(&prediction) {
                prediction.warnings.push(warning(
                    "FIBER_ROUTE_002",
                    format!(
                        "alternative route candidate exists via {}",
                        alternative.path.join(" -> ")
                    ),
                ));
            }
        }

        Ok(prediction)
    }

    /// Returns native prediction plus CCH bridge availability/economics statement.
    pub async fn compare_routes(
        &self,
        from: &str,
        to: &str,
        amount: &str,
        asset: &str,
    ) -> AppResult<RouteComparison> {
        self.compare_routes_with_aliases(from, to, amount, asset, None)
            .await
    }

    /// Compares native and CCH prediction while preserving scenario aliases.
    pub async fn compare_routes_with_aliases(
        &self,
        from: &str,
        to: &str,
        amount: &str,
        asset: &str,
        aliases: Option<&NodeAliasMap>,
    ) -> AppResult<RouteComparison> {
        let native_fiber = self
            .can_pay_with_aliases(from, to, amount, asset, aliases)
            .await?;
        let cch_bridged = cch_statement(asset);
        let (recommendation, reason) = recommendation(&native_fiber, &cch_bridged, asset);

        Ok(RouteComparison {
            native_fiber,
            cch_bridged,
            recommendation,
            reason,
        })
    }

    /// Returns the best alternative route already computed for a prediction.
    pub fn suggest_alternative(&self, prediction: &PaymentPrediction) -> Option<RankedPath> {
        prediction.alternative_routes.first().cloned()
    }

    /// Finds and scores local candidate paths from known live channel state.
    fn find_paths(
        &self,
        from: &NodeSnapshot,
        to: &NodeSnapshot,
        amount: u128,
        edges: &[RouteEdge],
    ) -> Vec<RankedPath> {
        find_candidate_paths(&from.pubkey, &to.pubkey, edges, 4)
            .into_iter()
            .map(|path| score_route(path, amount))
            .collect()
    }

    async fn collect_nodes(
        &self,
        config: &DevkitConfig,
        aliases: Option<&NodeAliasMap>,
    ) -> AppResult<Vec<NodeSnapshot>> {
        let mut nodes = Vec::new();
        for node in &config.nodes {
            let endpoint = node.rpc_endpoint();
            let rpc = FiberRpc::new(&endpoint)?;
            let info = node_info_with_retry(&rpc).await.map_err(|err| {
                app_error(format!("could not read node_info for {}: {err}", node.name))
            })?;
            nodes.push(NodeSnapshot {
                alias: aliases
                    .and_then(|aliases| alias_for_node(aliases, &node.name))
                    .unwrap_or_else(|| node.name.clone()),
                configured_name: node.name.clone(),
                endpoint,
                pubkey: info.pubkey,
            });
        }
        Ok(nodes)
    }

    async fn collect_edges(&self, snapshots: &[NodeSnapshot]) -> AppResult<Vec<RouteEdge>> {
        let mut edges = Vec::new();
        for snapshot in snapshots {
            let rpc = FiberRpc::new(&snapshot.endpoint)?;
            let channels = list_channels_with_retry(&rpc).await.map_err(|err| {
                app_error(format!(
                    "could not read channels for {}: {err}",
                    snapshot.configured_name
                ))
            })?;

            for channel in channels {
                if channel.state_name.as_deref() != Some("ChannelReady") {
                    continue;
                }
                let Some(peer_pubkey) = channel.pubkey else {
                    continue;
                };
                edges.push(RouteEdge {
                    from: snapshot.pubkey.clone(),
                    to: peer_pubkey,
                    outbound_capacity: channel.local_balance.unwrap_or(0),
                    enabled: channel.enabled.unwrap_or(true),
                    source: RouteDataSource::LocalBalances,
                });
            }
        }

        // If local balances are unavailable, fall back to public graph topology.
        // Graph capacity is not directional liquidity, so scoring caps confidence later.
        if edges.is_empty() {
            edges = self.collect_graph_edges(snapshots).await?;
        }

        Ok(edges)
    }

    async fn collect_graph_edges(&self, snapshots: &[NodeSnapshot]) -> AppResult<Vec<RouteEdge>> {
        let Some(first) = snapshots.first() else {
            return Ok(Vec::new());
        };
        let rpc = FiberRpc::new(&first.endpoint)?;
        let channels = match graph_channels_with_retry(&rpc).await {
            Ok(channels) => channels,
            Err(RpcError::Rpc { .. }) | Err(RpcError::InvalidResponse { .. }) => {
                return Ok(Vec::new());
            }
            Err(err) => return Err(app_error(format!("could not read graph_channels: {err}"))),
        };

        let known_pubkeys = snapshots
            .iter()
            .map(|snapshot| snapshot.pubkey.as_str())
            .collect::<Vec<_>>();
        let mut edges = Vec::new();
        for channel in channels {
            let (Some(node1), Some(node2)) = (channel.node1, channel.node2) else {
                continue;
            };
            if !known_pubkeys.contains(&node1.as_str()) || !known_pubkeys.contains(&node2.as_str())
            {
                continue;
            }
            let capacity = channel.capacity.unwrap_or(0);
            edges.push(RouteEdge {
                from: node1.clone(),
                to: node2.clone(),
                outbound_capacity: capacity,
                enabled: true,
                source: RouteDataSource::GraphTopology,
            });
            edges.push(RouteEdge {
                from: node2,
                to: node1,
                outbound_capacity: capacity,
                enabled: true,
                source: RouteDataSource::GraphTopology,
            });
        }
        Ok(edges)
    }
}

async fn node_info_with_retry(rpc: &FiberRpc) -> Result<NodeInfo, RpcError> {
    retry_read_only_rpc(|| rpc.node_info()).await
}

async fn list_channels_with_retry(rpc: &FiberRpc) -> Result<Vec<Channel>, RpcError> {
    retry_read_only_rpc(|| rpc.list_channels()).await
}

async fn graph_channels_with_retry(rpc: &FiberRpc) -> Result<Vec<GraphChannel>, RpcError> {
    retry_read_only_rpc(|| rpc.graph_channels()).await
}

async fn retry_read_only_rpc<T, Fut, Operation>(mut operation: Operation) -> Result<T, RpcError>
where
    Operation: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, RpcError>>,
{
    let mut last_error = None;
    for attempt in 0..12 {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(err) if is_transient_transport_error(&err) => {
                last_error = Some(err);
                let delay_ms = (100_u64.saturating_mul(2_u64.saturating_pow(attempt))).min(2_000);
                sleep(Duration::from_millis(delay_ms)).await;
            }
            Err(err) => return Err(err),
        }
    }

    Err(last_error.unwrap_or_else(|| RpcError::Http {
        message: "read-only RPC retry exhausted without an error".to_string(),
    }))
}

fn is_transient_transport_error(error: &RpcError) -> bool {
    match error {
        RpcError::Http { message } => {
            message.contains("error sending request")
                || message.contains("connection refused")
                || message.contains("deadline has elapsed")
                || message.contains("timed out")
        }
        _ => false,
    }
}

fn alias_for_node(aliases: &NodeAliasMap, node_name: &str) -> Option<String> {
    aliases
        .iter()
        .find_map(|(alias, name)| (name == node_name).then(|| alias.clone()))
}

fn find_candidate_paths(
    from: &str,
    to: &str,
    edges: &[RouteEdge],
    max_hops: usize,
) -> Vec<CandidatePath> {
    let mut queue = VecDeque::from([CandidatePath {
        nodes: vec![from.to_string()],
        edges: Vec::new(),
    }]);
    let mut paths = Vec::new();

    while let Some(path) = queue.pop_front() {
        let Some(current) = path.nodes.last() else {
            continue;
        };
        if current == to {
            paths.push(path);
            continue;
        }
        if path.edges.len() >= max_hops {
            continue;
        }

        for edge in edges
            .iter()
            .filter(|edge| edge.enabled && edge.from == *current)
        {
            if path.nodes.contains(&edge.to) {
                continue;
            }
            let mut next = path.clone();
            next.nodes.push(edge.to.clone());
            next.edges.push(edge.clone());
            queue.push_back(next);
        }
    }

    paths
}

fn score_route(path: CandidatePath, amount: u128) -> RankedPath {
    let hop_count = path.edges.len();
    let min_outbound_capacity = path
        .edges
        .iter()
        .map(|edge| edge.outbound_capacity)
        .min()
        .unwrap_or(0);
    let capacity_score = if amount == 0 {
        1.0
    } else {
        ratio(min_outbound_capacity, amount).min(1.0)
    };
    let hop_penalty = (1.0 - hop_count.saturating_sub(1) as f64 * 0.05).max(0.0);
    let channel_health = if path.edges.iter().all(|edge| edge.enabled) {
        1.0
    } else {
        0.0
    };
    let data_source = if path
        .edges
        .iter()
        .any(|edge| edge.source == RouteDataSource::GraphTopology)
    {
        RouteDataSource::GraphTopology
    } else {
        RouteDataSource::LocalBalances
    };
    let estimated_fee_shannons = estimate_fee(amount, hop_count);
    let fee_cost =
        (1.0 - ratio(estimated_fee_shannons, DEFAULT_FEE_TOLERANCE_SHANNONS)).clamp(0.0, 1.0);

    // This is a transparent heuristic from AGENT.md Section 9, not a statistical model.
    let mut probability = if min_outbound_capacity < amount {
        0.0
    } else {
        (0.40 * capacity_score) + (0.20 * hop_penalty) + (0.20 * channel_health) + (0.20 * fee_cost)
    }
    .clamp(0.0, 1.0);
    if data_source == RouteDataSource::GraphTopology {
        // Public graph capacity proves topology, not spendable directional balance.
        probability = probability.min(0.60);
    }

    RankedPath {
        path: path.nodes,
        probability: round_probability(probability),
        hop_count,
        capacity_score: round_probability(capacity_score),
        hop_penalty: round_probability(hop_penalty),
        channel_health: round_probability(channel_health),
        fee_cost: round_probability(fee_cost),
        min_outbound_capacity: format_ckb(min_outbound_capacity),
        estimated_fee: format_ckb(estimated_fee_shannons),
        data_source,
    }
}

fn prediction_warnings(
    ranked: &[RankedPath],
    amount: u128,
    from: &str,
    to: &str,
) -> Vec<PredictionWarning> {
    let mut warnings = Vec::new();
    if ranked.is_empty() {
        warnings.push(warning(
            "FIBER_ROUTE_001",
            format!("no known channel path from {from} to {to}"),
        ));
        return warnings;
    }

    let best = &ranked[0];
    let best_capacity = parse_formatted_ckb(&best.min_outbound_capacity).unwrap_or(0);
    if best_capacity < amount {
        warnings.push(warning(
            "FIBER_LIQ_001",
            format!(
                "best path has {} outbound capacity but payment requires {}",
                best.min_outbound_capacity,
                format_ckb(amount)
            ),
        ));
    }
    if best.data_source == RouteDataSource::GraphTopology {
        warnings.push(warning(
            "FIBER_ROUTE_002",
            "prediction used graph topology fallback; graph capacity is not directional liquidity, so confidence is capped".to_string(),
        ));
    }
    if best.hop_count > 2 {
        warnings.push(warning(
            "FIBER_ROUTE_002",
            "longer routes rely on graph freshness and every hop staying ready".to_string(),
        ));
    }
    warnings
}

fn warning(code: &str, message: String) -> PredictionWarning {
    let entry = entry_by_code(code);
    PredictionWarning {
        code: code.to_string(),
        category: entry
            .map(|entry| entry.category.to_string())
            .unwrap_or_else(|| "Unknown".to_string()),
        message,
    }
}

fn unsupported_native_asset_prediction(
    from: &str,
    to: &str,
    amount: &str,
    asset: &str,
) -> PaymentPrediction {
    PaymentPrediction {
        from: from.to_string(),
        to: to.to_string(),
        amount: amount.to_string(),
        asset: asset.to_string(),
        probability: 0.0,
        estimated_fee: format_ckb(0),
        estimated_latency: "unknown".to_string(),
        hop_count: 0,
        confidence: Confidence::Low,
        warnings: vec![warning(
            "FIBER_ASSET_001",
            format!(
                "native Demo 4 prediction only inspects CKB channels; `{asset}` requires explicit asset-aware channel data"
            ),
        )],
        best_route: None,
        alternative_routes: Vec::new(),
    }
}

fn cch_statement(asset: &str) -> CchPathResult {
    if !asset.eq_ignore_ascii_case("BTC") && !asset.eq_ignore_ascii_case("wrapped-BTC") {
        return CchPathResult::unavailable(
            "CCH only bridges BTC and wrapped-BTC; this prediction is for a native Fiber asset.",
        );
    }

    CchPathResult::unavailable(
        "No live CCH gateway/order probe is configured in the Demo 4 local DevKit network.",
    )
}

fn recommendation(
    native_fiber: &PaymentPrediction,
    cch_bridged: &CchPathResult,
    asset: &str,
) -> (RouteChoice, String) {
    if native_fiber.probability > 0.0 {
        return (
            RouteChoice::Native,
            "Native Fiber analysis completed; CCH is only relevant for BTC/wrapped-BTC bridge flows."
                .to_string(),
        );
    }

    if cch_bridged.available && asset.eq_ignore_ascii_case("BTC") {
        return (
            RouteChoice::CchBridged,
            "Native route confidence is zero and a CCH bridge is available for BTC.".to_string(),
        );
    }

    (
        RouteChoice::Unavailable,
        "Native Fiber route confidence is zero and CCH is unavailable for this request."
            .to_string(),
    )
}

fn snapshot_by_alias<'a>(
    snapshots: &'a [NodeSnapshot],
    alias: &str,
) -> AppResult<&'a NodeSnapshot> {
    snapshots
        .iter()
        .find(|snapshot| snapshot.alias == alias || snapshot.configured_name == alias)
        .ok_or_else(|| app_error(format!("unknown node alias `{alias}`")))
}

fn relabel_paths(paths: &mut [RankedPath], snapshots: &[NodeSnapshot]) {
    let labels = snapshots
        .iter()
        .map(|snapshot| (snapshot.pubkey.clone(), snapshot.alias.clone()))
        .collect::<HashMap<_, _>>();

    for path in paths {
        for node in &mut path.path {
            if let Some(alias) = labels.get(node) {
                *node = alias.clone();
            }
        }
    }
}

fn dedupe_ranked_paths(paths: &mut Vec<RankedPath>) {
    let mut seen = Vec::<Vec<String>>::new();
    paths.retain(|path| {
        if seen.iter().any(|existing| existing == &path.path) {
            false
        } else {
            seen.push(path.path.clone());
            true
        }
    });
}

fn normalize_amount(amount: &str) -> String {
    let trimmed = amount.trim();
    if trimmed.to_ascii_lowercase().ends_with("ckb") {
        trimmed.to_string()
    } else {
        format!("{trimmed} CKB")
    }
}

fn confidence(probability: f64) -> Confidence {
    if probability > 0.85 {
        Confidence::High
    } else if probability > 0.60 {
        Confidence::Medium
    } else {
        Confidence::Low
    }
}

fn estimate_fee(amount: u128, hop_count: usize) -> u128 {
    amount
        .saturating_mul(1_000)
        .saturating_div(1_000_000)
        .saturating_mul(hop_count as u128)
}

fn estimated_latency(hop_count: usize) -> String {
    if hop_count == 0 {
        "unknown".to_string()
    } else {
        format!("{}ms", hop_count * 18)
    }
}

fn ratio(numerator: u128, denominator: u128) -> f64 {
    if denominator == 0 {
        return 1.0;
    }
    numerator as f64 / denominator as f64
}

fn round_probability(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
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

fn parse_formatted_ckb(value: &str) -> Option<u128> {
    parse_ckb_amount(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::engine::DiagnosticEngine;

    fn edge(from: &str, to: &str, capacity: u128) -> RouteEdge {
        edge_with_source(from, to, capacity, RouteDataSource::LocalBalances)
    }

    fn edge_with_source(
        from: &str,
        to: &str,
        capacity: u128,
        source: RouteDataSource,
    ) -> RouteEdge {
        RouteEdge {
            from: from.to_string(),
            to: to.to_string(),
            outbound_capacity: capacity,
            enabled: true,
            source,
        }
    }

    #[test]
    fn scores_healthy_single_hop_as_high_confidence() {
        let path = CandidatePath {
            nodes: vec!["alice".to_string(), "bob".to_string()],
            edges: vec![edge("alice", "bob", 100_000_000_000)],
        };

        let ranked = score_route(path, 100_000_000);

        assert!(ranked.probability > 0.85);
        assert_eq!(ranked.hop_count, 1);
    }

    #[test]
    fn scores_low_liquidity_path_below_threshold() {
        let path = CandidatePath {
            nodes: vec!["alice".to_string(), "bob".to_string()],
            edges: vec![edge("alice", "bob", 50_000_000)],
        };

        let ranked = score_route(path, 100_000_000);

        assert!(ranked.probability < 0.2);
        assert!(ranked.capacity_score < 1.0);
    }

    #[test]
    fn finds_multi_hop_candidate_path() {
        let edges = vec![
            edge("alice", "hub", 100_000_000_000),
            edge("hub", "carol", 100_000_000_000),
        ];
        let paths = find_candidate_paths("alice", "carol", &edges, 4);

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].nodes, vec!["alice", "hub", "carol"]);
    }

    #[test]
    fn cch_statement_has_no_probability_fields() {
        let statement = cch_statement("CKB");
        let rendered = serde_json::to_value(statement).unwrap();

        assert!(rendered.get("probability").is_none());
        assert!(rendered.get("hopCount").is_none());
        assert_eq!(rendered["available"], false);
    }

    #[test]
    fn unsupported_native_asset_prediction_warns_without_scoring() {
        let prediction = unsupported_native_asset_prediction("alice", "bob", "1 CKB", "BTC");

        assert_eq!(prediction.probability, 0.0);
        assert_eq!(prediction.warnings[0].code, "FIBER_ASSET_001");
    }

    #[test]
    fn cch_btc_statement_is_unavailable_without_live_gateway() {
        let statement = cch_statement("BTC");

        assert!(!statement.available);
        assert!(statement.reason.contains("No live CCH gateway"));
    }

    #[test]
    fn dedupes_repeated_same_path_candidates() {
        let mut paths = vec![
            RankedPath {
                path: vec!["alice".to_string(), "bob".to_string()],
                probability: 0.9,
                hop_count: 1,
                capacity_score: 1.0,
                hop_penalty: 1.0,
                channel_health: 1.0,
                fee_cost: 1.0,
                min_outbound_capacity: "100 CKB".to_string(),
                estimated_fee: "0.001 CKB".to_string(),
                data_source: RouteDataSource::LocalBalances,
            },
            RankedPath {
                path: vec!["alice".to_string(), "bob".to_string()],
                probability: 0.8,
                hop_count: 1,
                capacity_score: 1.0,
                hop_penalty: 1.0,
                channel_health: 1.0,
                fee_cost: 1.0,
                min_outbound_capacity: "100 CKB".to_string(),
                estimated_fee: "0.001 CKB".to_string(),
                data_source: RouteDataSource::LocalBalances,
            },
        ];

        dedupe_ranked_paths(&mut paths);

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].probability, 0.9);
    }

    #[test]
    fn bare_cli_amounts_are_treated_as_ckb() {
        assert_eq!(normalize_amount("1"), "1 CKB");
        assert_eq!(normalize_amount("0.5 CKB"), "0.5 CKB");
    }

    #[test]
    fn warning_uses_taxonomy_metadata() {
        let warning = warning("FIBER_LIQ_001", "low".to_string());

        assert_eq!(warning.category, "Liquidity");
    }

    #[test]
    fn confidence_thresholds_match_agent_spec() {
        assert_eq!(confidence(0.86), Confidence::High);
        assert_eq!(confidence(0.61), Confidence::Medium);
        assert_eq!(confidence(0.60), Confidence::Low);
    }

    #[test]
    fn graph_topology_fallback_caps_confidence_and_warns() {
        let path = CandidatePath {
            nodes: vec!["alice".to_string(), "bob".to_string()],
            edges: vec![edge_with_source(
                "alice",
                "bob",
                100_000_000_000,
                RouteDataSource::GraphTopology,
            )],
        };

        let ranked = score_route(path, 100_000_000);
        let warnings = prediction_warnings(&[ranked.clone()], 100_000_000, "alice", "bob");

        assert_eq!(ranked.data_source, RouteDataSource::GraphTopology);
        assert_eq!(ranked.probability, 0.60);
        assert_eq!(confidence(ranked.probability), Confidence::Low);
        assert!(warnings.iter().any(|warning| {
            warning.code == "FIBER_ROUTE_002" && warning.message.contains("graph topology")
        }));
    }

    #[test]
    fn diagnostic_engine_still_compiles_with_route_module() {
        let report = DiagnosticEngine::new().diagnose_text("no route to target");

        assert_eq!(report.error_code, "FIBER_ROUTE_001");
    }
}
