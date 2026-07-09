//! Scenario data model for Fiber DevKit demos.
//! Owns the exact MVP schema accepted by `SCENARIO_FORMAT.md`; report artifacts
//! and CI integration consume these results but live outside this layer.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::NodeTemplate;

/// Parsed YAML scenario ready for execution.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub name: String,
    pub description: Option<String>,
    pub nodes: BTreeMap<String, ScenarioNode>,
    #[serde(default)]
    pub channels: Vec<ChannelSetup>,
    pub steps: Vec<Step>,
    #[serde(default = "default_assertions")]
    pub assertions: Vec<Assertion>,
    #[serde(skip)]
    pub source_path: Option<PathBuf>,
}

/// Scenario alias mapped to a live configured node or explicit endpoint.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioNode {
    pub node: Option<String>,
    pub endpoint: Option<String>,
    pub template: Option<NodeTemplate>,
}

/// Channel opened before scenario steps execute.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelSetup {
    pub from: String,
    pub to: String,
    pub capacity: String,
    #[serde(default = "default_true")]
    pub public: bool,
    /// Initiator-funded channels must be private because FNN rejects public one-way channels.
    #[serde(default)]
    pub one_way: bool,
}

/// Expected result for an executable step.
#[derive(Debug, Clone, Copy, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Expectation {
    #[default]
    Success,
    Failure,
}

/// Scenario action variants accepted by the MVP parser.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    NodeInfo {
        node: String,
        #[serde(default)]
        expect: Expectation,
    },
    ListChannels {
        node: String,
        #[serde(default)]
        expect: Expectation,
    },
    OpenChannel {
        from: String,
        to: String,
        capacity: String,
        #[serde(default = "default_true")]
        public: bool,
        /// Initiator-funded channels must be private because FNN rejects public one-way channels.
        #[serde(default)]
        one_way: bool,
        #[serde(default)]
        expect: Expectation,
    },
    Pay {
        from: String,
        to: String,
        amount: String,
        #[serde(default)]
        expect: Expectation,
        #[serde(default = "default_payment_timeout")]
        timeout_seconds: u64,
        max_fee: Option<String>,
        #[serde(default)]
        dry_run: bool,
    },
    Predict {
        from: String,
        to: String,
        amount: String,
        #[serde(default = "default_asset")]
        asset: String,
        #[serde(default)]
        cross_chain: bool,
        #[serde(default)]
        expect: Expectation,
        expect_probability_above: Option<f64>,
        expect_probability_below: Option<f64>,
    },
    GraphNodes {
        node: String,
        #[serde(default)]
        expect: Expectation,
    },
    GraphChannels {
        node: String,
        #[serde(default)]
        expect: Expectation,
    },
}

/// Scenario-level assertions accepted by the MVP.
#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Assertion {
    AllStepsPassed,
}

/// Outcome observed after running one setup item or step.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StepResult {
    pub index: usize,
    pub name: String,
    pub action: String,
    pub expect: Expectation,
    pub status: StepStatus,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

/// Pass/fail state for a step after expectation matching.
#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Passed,
    Failed,
}

/// Complete result for a scenario run.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RunResult {
    pub scenario: String,
    pub description: Option<String>,
    pub passed: bool,
    pub steps: Vec<StepResult>,
    pub assertions: Vec<AssertionResult>,
}

/// Result of evaluating one scenario assertion.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AssertionResult {
    pub assertion: Assertion,
    pub passed: bool,
    pub message: String,
}

/// Final summary printed after individual step JSON lines.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RunSummary {
    pub scenario: String,
    pub description: Option<String>,
    pub passed: bool,
    pub steps_total: usize,
    pub steps_passed: usize,
    pub steps_failed: usize,
    pub assertions: Vec<AssertionResult>,
}

impl Step {
    /// Returns the expected outcome for expectation matching.
    pub fn expectation(&self) -> Expectation {
        match self {
            Self::NodeInfo { expect, .. }
            | Self::ListChannels { expect, .. }
            | Self::OpenChannel { expect, .. }
            | Self::Pay { expect, .. }
            | Self::Predict { expect, .. }
            | Self::GraphNodes { expect, .. }
            | Self::GraphChannels { expect, .. } => *expect,
        }
    }

    /// Returns a stable action name for step-level output.
    pub fn action_name(&self) -> &'static str {
        match self {
            Self::NodeInfo { .. } => "node_info",
            Self::ListChannels { .. } => "list_channels",
            Self::OpenChannel { .. } => "open_channel",
            Self::Pay { .. } => "pay",
            Self::Predict { .. } => "predict",
            Self::GraphNodes { .. } => "graph_nodes",
            Self::GraphChannels { .. } => "graph_channels",
        }
    }
}

impl RunResult {
    /// Builds the final compact pass/fail summary object.
    pub fn summary(&self) -> RunSummary {
        let steps_passed = self
            .steps
            .iter()
            .filter(|step| step.status == StepStatus::Passed)
            .count();
        let steps_total = self.steps.len();

        RunSummary {
            scenario: self.scenario.clone(),
            description: self.description.clone(),
            passed: self.passed,
            steps_total,
            steps_passed,
            steps_failed: steps_total.saturating_sub(steps_passed),
            assertions: self.assertions.clone(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_payment_timeout() -> u64 {
    30
}

fn default_asset() -> String {
    "CKB".to_string()
}

fn default_assertions() -> Vec<Assertion> {
    vec![Assertion::AllStepsPassed]
}
