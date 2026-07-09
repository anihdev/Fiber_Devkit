//! Scenario runner for Fiber DevKit demos.
//! Owns step ordering, expectation matching, setup waits, route prediction steps,
//! and diagnosis metadata; taxonomy matching and prediction scoring live in their
//! own modules. It does not implement report artifacts.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::time::sleep;

use crate::config::DevkitConfig;
use crate::diagnostic::engine::DiagnosticEngine;
use crate::route::analyzer::{NodeAliasMap, PaymentPrediction, RouteAnalyzer};
use crate::rpc::client::FiberRpc;
use crate::rpc::types::{
    Channel, OpenChannelParams, OpenChannelResult, Payment, RpcError, SendPaymentParams,
};
use crate::scenario::parser::parse_ckb_amount;
use crate::scenario::types::{
    Assertion, AssertionResult, ChannelSetup, Expectation, RunResult, Scenario, ScenarioNode, Step,
    StepResult, StepStatus,
};
use crate::{app_error, AppResult};

/// Executes parsed scenarios against the current `.fiber/config.toml`.
pub struct ScenarioRunner {
    project_root: PathBuf,
}

#[derive(Debug, Clone)]
struct ResolvedNode {
    endpoint: String,
    configured_name: Option<String>,
}

struct PaymentStepInput<'a> {
    from: &'a str,
    to: &'a str,
    amount: &'a str,
    timeout_seconds: u64,
    max_fee: Option<&'a str>,
    dry_run: bool,
}

struct PredictStepInput<'a> {
    from: &'a str,
    to: &'a str,
    amount: &'a str,
    asset: &'a str,
    cross_chain: bool,
    expect_probability_above: Option<f64>,
    expect_probability_below: Option<f64>,
}

impl ScenarioRunner {
    /// Creates a runner bound to a project root.
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }

    /// Runs setup channels and scenario steps in order, collecting structured results.
    pub async fn run(&self, scenario: Scenario) -> AppResult<RunResult> {
        let config = DevkitConfig::read_from_project(&self.project_root)?;
        let nodes = self.resolve_nodes(&scenario, &config)?;
        let mut results = Vec::new();
        let mut next_index = 1;

        // Channel setup is part of the observable run because it can fail due funding/liquidity.
        for channel in &scenario.channels {
            let result = self
                .run_channel_setup(next_index, channel, &nodes, true)
                .await;
            results.push(result);
            next_index += 1;
        }

        for step in &scenario.steps {
            let result = self.run_step(next_index, step, &nodes).await;
            results.push(result);
            next_index += 1;
        }

        // The MVP assertion set stays intentionally small so new actions remain composable.
        let assertions = scenario
            .assertions
            .iter()
            .map(|assertion| evaluate_assertion(assertion, &results))
            .collect::<Vec<_>>();
        let passed = assertions.iter().all(|assertion| assertion.passed);

        Ok(RunResult {
            scenario: scenario.name,
            description: scenario.description,
            passed,
            steps: results,
            assertions,
        })
    }

    fn resolve_nodes(
        &self,
        scenario: &Scenario,
        config: &DevkitConfig,
    ) -> AppResult<BTreeMap<String, ResolvedNode>> {
        let mut nodes = BTreeMap::new();
        for (alias, node) in &scenario.nodes {
            let resolved = self.resolve_node(alias, node, config)?;
            nodes.insert(alias.clone(), resolved);
        }
        Ok(nodes)
    }

    fn resolve_node(
        &self,
        alias: &str,
        node: &ScenarioNode,
        config: &DevkitConfig,
    ) -> AppResult<ResolvedNode> {
        if let Some(endpoint) = &node.endpoint {
            return Ok(ResolvedNode {
                endpoint: endpoint.clone(),
                configured_name: None,
            });
        }

        let name = node
            .node
            .as_ref()
            .ok_or_else(|| app_error(format!("node alias `{alias}` did not define node")))?;
        let configured = config
            .nodes
            .iter()
            .find(|configured| configured.name == *name)
            .ok_or_else(|| {
                app_error(format!("node alias `{alias}` references unknown `{name}`"))
            })?;

        if let Some(expected) = node.template {
            if configured.template != expected {
                return Err(app_error(format!(
                    "node alias `{alias}` expected template `{expected}` but `{name}` is `{}`",
                    configured.template
                )));
            }
        }

        Ok(ResolvedNode {
            endpoint: configured.rpc_endpoint(),
            configured_name: Some(configured.name.clone()),
        })
    }

    async fn run_channel_setup(
        &self,
        index: usize,
        channel: &ChannelSetup,
        nodes: &BTreeMap<String, ResolvedNode>,
        wait_ready: bool,
    ) -> StepResult {
        let expectation = Expectation::Success;
        let observed = match self
            .open_channel_typed(
                &channel.from,
                &channel.to,
                &channel.capacity,
                channel.public,
                channel.one_way,
                nodes,
            )
            .await
        {
            Ok(value) => {
                if wait_ready {
                    self.wait_for_channel_ready(&channel.from, &channel.to, &value, nodes)
                        .await
                        .map(|_| serde_json::to_value(value).unwrap_or(Value::Null))
                } else {
                    Ok(serde_json::to_value(value).unwrap_or(Value::Null))
                }
            }
            Err(err) => Err(err),
        };

        step_result(
            index,
            format!("setup channel {} -> {}", channel.from, channel.to),
            "open_channel",
            expectation,
            observed,
        )
    }

    async fn run_step(
        &self,
        index: usize,
        step: &Step,
        nodes: &BTreeMap<String, ResolvedNode>,
    ) -> StepResult {
        let expectation = step.expectation();
        let observed = match step {
            Step::NodeInfo { node, .. } => self.node_info(node, nodes).await,
            Step::ListChannels { node, .. } => self.list_channels(node, nodes).await,
            Step::OpenChannel {
                from,
                to,
                capacity,
                public,
                one_way,
                ..
            } => {
                self.open_channel_between(from, to, capacity, *public, *one_way, nodes)
                    .await
            }
            Step::Pay {
                from,
                to,
                amount,
                timeout_seconds,
                max_fee,
                dry_run,
                ..
            } => {
                self.pay(
                    PaymentStepInput {
                        from,
                        to,
                        amount,
                        timeout_seconds: *timeout_seconds,
                        max_fee: max_fee.as_deref(),
                        dry_run: *dry_run,
                    },
                    nodes,
                )
                .await
            }
            Step::Predict {
                from,
                to,
                amount,
                asset,
                cross_chain,
                expect_probability_above,
                expect_probability_below,
                ..
            } => {
                self.predict(
                    PredictStepInput {
                        from,
                        to,
                        amount,
                        asset,
                        cross_chain: *cross_chain,
                        expect_probability_above: *expect_probability_above,
                        expect_probability_below: *expect_probability_below,
                    },
                    nodes,
                )
                .await
            }
            Step::GraphNodes { node, .. } => self.graph_nodes(node, nodes).await,
            Step::GraphChannels { node, .. } => self.graph_channels(node, nodes).await,
        };

        step_result(
            index,
            step.action_name().to_string(),
            step.action_name(),
            expectation,
            observed,
        )
    }

    async fn node_info(
        &self,
        alias: &str,
        nodes: &BTreeMap<String, ResolvedNode>,
    ) -> Result<Value, RpcError> {
        let node = lookup_node(alias, nodes)?;
        let rpc = FiberRpc::new(&node.endpoint)?;
        let info = rpc.node_info().await?;
        Ok(serde_json::to_value(info).unwrap_or(Value::Null))
    }

    async fn list_channels(
        &self,
        alias: &str,
        nodes: &BTreeMap<String, ResolvedNode>,
    ) -> Result<Value, RpcError> {
        let node = lookup_node(alias, nodes)?;
        let rpc = FiberRpc::new(&node.endpoint)?;
        let channels = rpc.list_channels().await?;
        Ok(serde_json::json!({ "count": channels.len(), "channels": channels }))
    }

    async fn open_channel_between(
        &self,
        from: &str,
        to: &str,
        capacity: &str,
        public: bool,
        one_way: bool,
        nodes: &BTreeMap<String, ResolvedNode>,
    ) -> Result<Value, RpcError> {
        let result = self
            .open_channel_typed(from, to, capacity, public, one_way, nodes)
            .await?;
        Ok(serde_json::to_value(result).unwrap_or(Value::Null))
    }

    async fn open_channel_typed(
        &self,
        from: &str,
        to: &str,
        capacity: &str,
        public: bool,
        one_way: bool,
        nodes: &BTreeMap<String, ResolvedNode>,
    ) -> Result<OpenChannelResult, RpcError> {
        let from_node = lookup_node(from, nodes)?;
        let to_node = lookup_node(to, nodes)?;
        let target_pubkey = self.get_pubkey(to_node).await?;
        let funding_amount =
            parse_ckb_amount(capacity).map_err(|err| RpcError::InvalidResponse {
                message: err.to_string(),
                raw: None,
            })?;
        let rpc = FiberRpc::new(&from_node.endpoint)?;
        rpc.open_channel(OpenChannelParams {
            pubkey: target_pubkey,
            funding_amount,
            public,
            one_way,
        })
        .await
    }

    async fn pay(
        &self,
        input: PaymentStepInput<'_>,
        nodes: &BTreeMap<String, ResolvedNode>,
    ) -> Result<Value, RpcError> {
        let from_node = lookup_node(input.from, nodes)?;
        let to_node = lookup_node(input.to, nodes)?;
        let target_pubkey = self.get_pubkey(to_node).await?;
        let amount = parse_ckb_amount(input.amount).map_err(|err| RpcError::InvalidResponse {
            message: err.to_string(),
            raw: None,
        })?;
        let max_fee_amount = input
            .max_fee
            .map(parse_ckb_amount)
            .transpose()
            .map_err(|err| RpcError::InvalidResponse {
                message: err.to_string(),
                raw: None,
            })?;

        let rpc = FiberRpc::new(&from_node.endpoint)?;
        let initial = rpc
            .send_payment(SendPaymentParams {
                target_pubkey,
                amount,
                timeout_seconds: input.timeout_seconds,
                max_fee_amount,
                dry_run: input.dry_run,
            })
            .await?;

        let payment = if input.dry_run {
            initial
        } else {
            self.wait_for_payment(&rpc, initial).await?
        };

        if payment.is_success() || input.dry_run {
            Ok(serde_json::to_value(payment).unwrap_or(Value::Null))
        } else {
            let message = payment
                .failed_error
                .clone()
                .unwrap_or_else(|| format!("payment ended with status {:?}", payment.status));
            Err(RpcError::Rpc {
                code: -32000,
                message,
                data: Some(serde_json::to_value(payment).unwrap_or(Value::Null)),
            })
        }
    }

    async fn graph_nodes(
        &self,
        alias: &str,
        nodes: &BTreeMap<String, ResolvedNode>,
    ) -> Result<Value, RpcError> {
        let node = lookup_node(alias, nodes)?;
        let rpc = FiberRpc::new(&node.endpoint)?;
        let graph_nodes = rpc.graph_nodes().await?;
        Ok(serde_json::json!({ "count": graph_nodes.len(), "nodes": graph_nodes }))
    }

    async fn predict(
        &self,
        input: PredictStepInput<'_>,
        nodes: &BTreeMap<String, ResolvedNode>,
    ) -> Result<Value, RpcError> {
        let aliases = scenario_aliases_to_config_nodes(nodes);
        let analyzer = RouteAnalyzer::new(self.project_root.clone());
        let prediction_value = if input.cross_chain {
            let comparison = analyzer
                .compare_routes_with_aliases(
                    input.from,
                    input.to,
                    input.amount,
                    input.asset,
                    Some(&aliases),
                )
                .await
                .map_err(app_error_to_rpc)?;
            let probability = comparison.native_fiber.probability;
            validate_prediction_bounds(
                probability,
                input.expect_probability_above,
                input.expect_probability_below,
            )?;
            serde_json::to_value(comparison).unwrap_or(Value::Null)
        } else {
            let prediction = analyzer
                .can_pay_with_aliases(
                    input.from,
                    input.to,
                    input.amount,
                    input.asset,
                    Some(&aliases),
                )
                .await
                .map_err(app_error_to_rpc)?;
            validate_prediction_bounds(
                prediction.probability,
                input.expect_probability_above,
                input.expect_probability_below,
            )?;
            prediction_to_value(prediction)
        };

        Ok(prediction_value)
    }

    async fn graph_channels(
        &self,
        alias: &str,
        nodes: &BTreeMap<String, ResolvedNode>,
    ) -> Result<Value, RpcError> {
        let node = lookup_node(alias, nodes)?;
        let rpc = FiberRpc::new(&node.endpoint)?;
        let graph_channels = rpc.graph_channels().await?;
        Ok(serde_json::json!({ "count": graph_channels.len(), "channels": graph_channels }))
    }

    async fn get_pubkey(&self, node: &ResolvedNode) -> Result<String, RpcError> {
        let rpc = FiberRpc::new(&node.endpoint)?;
        Ok(rpc.node_info().await?.pubkey)
    }

    async fn wait_for_channel_ready(
        &self,
        from: &str,
        to: &str,
        opened: &OpenChannelResult,
        nodes: &BTreeMap<String, ResolvedNode>,
    ) -> Result<(), RpcError> {
        let from_node = lookup_node(from, nodes)?;
        let to_node = lookup_node(to, nodes)?;
        let to_pubkey = self.get_pubkey(to_node).await?;
        let rpc = FiberRpc::new(&from_node.endpoint)?;
        let timeout = Duration::from_secs(180);
        let deadline = Instant::now() + timeout;
        let mut last_channels = Vec::<Channel>::new();

        // Channel opening involves funding collaboration and CKB confirmation before routing works.
        while Instant::now() < deadline {
            let channels = rpc.list_channels().await?;
            if channels
                .iter()
                .any(|channel| channel.is_ready_with(&to_pubkey))
            {
                return Ok(());
            }
            if let Some(closed) = channels
                .iter()
                .filter(|channel| channel_matches_opened(channel, opened))
                .find(|channel| channel.is_closed_with(&to_pubkey))
            {
                let reason = channel_failure_detail(closed)
                    .map(|detail| format!(": {detail}"))
                    .unwrap_or_default();
                return Err(RpcError::InvalidResponse {
                    message: format!("channel {from}->{to} closed before becoming ready{reason}"),
                    raw: Some(serde_json::to_value(closed).unwrap_or(Value::Null)),
                });
            }
            let pending = rpc.list_pending_channels().await.unwrap_or_default();
            if let Some(closed) = pending
                .iter()
                .filter(|channel| channel_matches_opened(channel, opened))
                .find(|channel| channel.is_closed_with(&to_pubkey))
            {
                let reason = channel_failure_detail(closed)
                    .map(|detail| format!(": {detail}"))
                    .unwrap_or_default();
                return Err(RpcError::InvalidResponse {
                    message: format!("channel {from}->{to} closed before becoming ready{reason}"),
                    raw: Some(serde_json::to_value(closed).unwrap_or(Value::Null)),
                });
            }
            last_channels = if pending.is_empty() {
                channels
            } else {
                pending
            };
            sleep(Duration::from_secs(3)).await;
        }

        let closed = rpc.list_all_channels().await.unwrap_or_default();
        if let Some(closed) = closed
            .iter()
            .filter(|channel| channel_matches_opened(channel, opened))
            .find(|channel| channel.is_closed_with(&to_pubkey))
        {
            let reason = channel_failure_detail(closed)
                .map(|detail| format!(": {detail}"))
                .unwrap_or_default();
            return Err(RpcError::InvalidResponse {
                message: format!("channel {from}->{to} closed before becoming ready{reason}"),
                raw: Some(serde_json::to_value(closed).unwrap_or(Value::Null)),
            });
        } else if !closed.is_empty() {
            last_channels = closed;
        }

        Err(RpcError::InvalidResponse {
            message: format!(
                "channel {from}->{to} did not become ready within {}s",
                timeout.as_secs()
            ),
            raw: Some(serde_json::to_value(last_channels).unwrap_or(Value::Null)),
        })
    }

    async fn wait_for_payment(
        &self,
        rpc: &FiberRpc,
        initial: Payment,
    ) -> Result<Payment, RpcError> {
        if initial.is_success() || initial.is_failed() {
            return Ok(initial);
        }

        let Some(payment_hash) = initial.payment_hash.clone() else {
            return Ok(initial);
        };

        let deadline = Instant::now() + Duration::from_secs(45);
        let mut current = initial;

        // `send_payment` may return before settlement; poll by hash until terminal or timeout.
        while Instant::now() < deadline {
            current = rpc.get_payment(&payment_hash).await?;
            if current.is_success() || current.is_failed() {
                return Ok(current);
            }
            sleep(Duration::from_secs(2)).await;
        }

        Ok(current)
    }
}

fn lookup_node<'a>(
    alias: &str,
    nodes: &'a BTreeMap<String, ResolvedNode>,
) -> Result<&'a ResolvedNode, RpcError> {
    nodes.get(alias).ok_or_else(|| RpcError::InvalidResponse {
        message: format!("unknown resolved node alias `{alias}`"),
        raw: None,
    })
}

fn scenario_aliases_to_config_nodes(nodes: &BTreeMap<String, ResolvedNode>) -> NodeAliasMap {
    nodes
        .iter()
        .filter_map(|(alias, node)| {
            node.configured_name
                .as_ref()
                .map(|configured| (alias.clone(), configured.clone()))
        })
        .collect()
}

fn validate_prediction_bounds(
    probability: f64,
    above: Option<f64>,
    below: Option<f64>,
) -> Result<(), RpcError> {
    if let Some(threshold) = above {
        if probability <= threshold {
            return Err(RpcError::InvalidResponse {
                message: format!(
                    "prediction probability {probability:.2} was not above {threshold:.2}"
                ),
                raw: None,
            });
        }
    }
    if let Some(threshold) = below {
        if probability >= threshold {
            return Err(RpcError::InvalidResponse {
                message: format!(
                    "prediction probability {probability:.2} was not below {threshold:.2}"
                ),
                raw: None,
            });
        }
    }
    Ok(())
}

fn prediction_to_value(prediction: PaymentPrediction) -> Value {
    serde_json::to_value(prediction).unwrap_or(Value::Null)
}

fn app_error_to_rpc(error: Box<dyn std::error::Error + Send + Sync>) -> RpcError {
    RpcError::InvalidResponse {
        message: error.to_string(),
        raw: None,
    }
}

fn channel_failure_detail(channel: &Channel) -> Option<String> {
    channel
        .raw
        .get("failure_detail")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn channel_matches_opened(channel: &Channel, opened: &OpenChannelResult) -> bool {
    match (&channel.channel_id, &opened.temporary_channel_id) {
        (Some(channel_id), Some(opened_id)) => channel_id == opened_id,
        (_, None) => true,
        _ => false,
    }
}

fn step_result(
    index: usize,
    name: String,
    action: &str,
    expectation: Expectation,
    observed: Result<Value, RpcError>,
) -> StepResult {
    let observed_success = observed.is_ok();
    let expected_success = expectation == Expectation::Success;
    let passed = observed_success == expected_success;

    match observed {
        Ok(details) => StepResult {
            index,
            name,
            action: action.to_string(),
            expect: expectation,
            status: if passed {
                StepStatus::Passed
            } else {
                StepStatus::Failed
            },
            message: if passed {
                "step succeeded as expected".to_string()
            } else {
                "step succeeded but expected failure".to_string()
            },
            details: Some(details),
        },
        Err(err) => {
            let diagnostic = DiagnosticEngine::new().diagnose_rpc_error(&err);
            let mut details = serde_json::to_value(&err).unwrap_or(Value::Null);
            if let Value::Object(object) = &mut details {
                object.insert(
                    "diagnosis".to_string(),
                    serde_json::to_value(diagnostic).unwrap_or(Value::Null),
                );
            }
            StepResult {
                index,
                name,
                action: action.to_string(),
                expect: expectation,
                status: if passed {
                    StepStatus::Passed
                } else {
                    StepStatus::Failed
                },
                message: if passed {
                    format!("step failed as expected: {}", err.message())
                } else {
                    format!("step failed: {}", err.message())
                },
                details: Some(details),
            }
        }
    }
}

fn evaluate_assertion(assertion: &Assertion, steps: &[StepResult]) -> AssertionResult {
    match assertion {
        Assertion::AllStepsPassed => {
            let failed = steps
                .iter()
                .filter(|step| step.status == StepStatus::Failed)
                .count();
            AssertionResult {
                assertion: assertion.clone(),
                passed: failed == 0,
                message: if failed == 0 {
                    "all steps matched expectations".to_string()
                } else {
                    format!("{failed} step(s) did not match expectations")
                },
            }
        }
    }
}
