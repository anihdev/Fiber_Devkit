//! Demo 5 report artifact writers.
//! Owns markdown, JSON log, and OTel-compatible trace files generated from real
//! scenario results; graph rendering and live OTel export remain roadmap.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config::FIBER_DIR;
use crate::scenario::types::{RunResult, StepResult, StepStatus};
use crate::tracer::events::trace_from_run;
use crate::AppResult;

/// Directory under `.fiber` where report artifacts are written.
pub const OUTPUT_DIR: &str = "output";
/// Persisted run result used by `fiber report`.
pub const LAST_RUN_FILE: &str = "last-run.json";

/// Paths written by report generation.
#[derive(Debug, Clone)]
pub struct ReportArtifacts {
    pub output_dir: PathBuf,
    pub report_md: PathBuf,
    pub logs_json: PathBuf,
    pub trace_json: PathBuf,
    pub last_run_json: PathBuf,
}

/// Writes report artifacts from a completed scenario run.
pub struct Reporter {
    project_root: PathBuf,
}

impl Reporter {
    /// Creates a reporter bound to the current project root.
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }

    /// Writes `report.md`, `logs.json`, `trace.json`, and the persisted last run.
    pub fn write_all(&self, result: &RunResult) -> AppResult<ReportArtifacts> {
        let artifacts = self.artifacts();
        fs::create_dir_all(&artifacts.output_dir)?;
        fs::write(&artifacts.report_md, self.markdown(result))?;
        fs::write(&artifacts.logs_json, serde_json::to_string_pretty(result)?)?;
        fs::write(
            &artifacts.trace_json,
            serde_json::to_string_pretty(&trace_from_run(result))?,
        )?;
        fs::write(
            &artifacts.last_run_json,
            serde_json::to_string_pretty(result)?,
        )?;
        Ok(artifacts)
    }

    /// Persists the most recent run so `fiber report` can regenerate artifacts later.
    pub fn persist_last_run(&self, result: &RunResult) -> AppResult<()> {
        let artifacts = self.artifacts();
        fs::create_dir_all(&artifacts.output_dir)?;
        fs::write(
            &artifacts.last_run_json,
            serde_json::to_string_pretty(result)?,
        )?;
        Ok(())
    }

    /// Rewrites artifacts from `.fiber/output/last-run.json`.
    pub fn write_from_last_run(&self) -> AppResult<ReportArtifacts> {
        let result = self.read_last_run()?;
        self.write_all(&result)
    }

    /// Reads the most recent persisted scenario result.
    pub fn read_last_run(&self) -> AppResult<RunResult> {
        let raw = fs::read_to_string(self.artifacts().last_run_json)?;
        Ok(serde_json::from_str(&raw)?)
    }

    /// Returns the deterministic output paths for this project.
    pub fn artifacts(&self) -> ReportArtifacts {
        let output_dir = output_dir(&self.project_root);
        ReportArtifacts {
            report_md: output_dir.join("report.md"),
            logs_json: output_dir.join("logs.json"),
            trace_json: output_dir.join("trace.json"),
            last_run_json: output_dir.join(LAST_RUN_FILE),
            output_dir,
        }
    }

    fn markdown(&self, result: &RunResult) -> String {
        let mut output = String::new();
        output.push_str("# Fiber DevKit Scenario Report\n\n");
        output.push_str(&format!("**Scenario:** `{}`\n\n", result.scenario));
        if let Some(description) = &result.description {
            output.push_str(&format!("**Description:** {description}\n\n"));
        }
        output.push_str(&format!(
            "**Status:** {}\n\n",
            if result.passed { "PASS" } else { "FAIL" }
        ));

        let summary = result.summary();
        output.push_str("## Summary\n\n");
        output.push_str("| Metric | Value |\n|---|---|\n");
        output.push_str(&format!("| Steps | {} |\n", summary.steps_total));
        output.push_str(&format!("| Passed | {} |\n", summary.steps_passed));
        output.push_str(&format!("| Failed | {} |\n", summary.steps_failed));
        output.push('\n');

        output.push_str("## Outcome\n\n");
        if result.passed {
            output.push_str(
                "All steps and assertions matched their declared expectations. No corrective action is required.\n\n",
            );
        } else {
            output.push_str(&format!(
                "The scenario failed because {} step(s) did not match their declared expectations. Review **Failure Analysis**, apply the recommended actions, and rerun the scenario with `--report`.\n\n",
                summary.steps_failed
            ));
        }

        output.push_str("## Step Results\n\n");
        output.push_str("| # | Action | Expected | Status | Message |\n|---|---|---|---|---|\n");
        for step in &result.steps {
            output.push_str(&format!(
                "| {} | `{}` | `{}` | {} | {} |\n",
                step.index,
                step.action,
                format!("{:?}", step.expect).to_lowercase(),
                status_label(step.status),
                escape_markdown_table(&step.message)
            ));
        }
        output.push('\n');

        let failed_steps = result
            .steps
            .iter()
            .filter(|step| step.status == StepStatus::Failed)
            .collect::<Vec<_>>();
        if !failed_steps.is_empty() {
            output.push_str("## Failure Analysis\n\n");
            output.push_str(
                "These are unexpected outcome mismatches that caused the scenario to fail.\n\n",
            );

            let groups = diagnosis_groups(failed_steps.iter().copied());
            for group in &groups {
                render_diagnosis_group(&mut output, group, "What to do next");
            }

            for step in failed_steps
                .iter()
                .copied()
                .filter(|step| step_diagnosis(step).is_none())
            {
                render_expectation_mismatch(&mut output, step);
            }
        } else if !result.passed {
            output.push_str("## Failure Analysis\n\n");
            output.push_str(
                "No step-level failure was recorded. Review the failed assertions below and inspect `logs.json` for the complete structured run result.\n\n",
            );
        }

        let expected_failure_groups = diagnosis_groups(result.steps.iter().filter(|step| {
            step.status == StepStatus::Passed
                && step.expect == crate::scenario::types::Expectation::Failure
        }));
        if !expected_failure_groups.is_empty() {
            output.push_str("## Expected Failure Analysis\n\n");
            output.push_str(
                "These failures were declared by the scenario and occurred as expected, so they did not fail the run. The guidance below applies if the same condition is unexpected.\n\n",
            );
            for group in &expected_failure_groups {
                render_diagnosis_group(&mut output, group, "What to do if unexpected");
            }
        }

        let predictions = result
            .steps
            .iter()
            .filter_map(step_prediction_summary)
            .collect::<Vec<_>>();
        if !predictions.is_empty() {
            output.push_str("## Predictions\n\n");
            for prediction in predictions {
                output.push_str(&format!("- {prediction}\n"));
            }
            output.push('\n');
        }

        output.push_str("## Assertions\n\n");
        for assertion in &result.assertions {
            output.push_str(&format!(
                "- `{:?}`: {} — {}\n",
                assertion.assertion,
                if assertion.passed { "passed" } else { "failed" },
                assertion.message
            ));
        }
        output.push('\n');
        output.push_str("## Artifacts\n\n");
        output.push_str("- `logs.json`: full structured run result\n");
        output.push_str("- `trace.json`: OTel-compatible span data\n");
        output
    }
}

/// Returns `.fiber/output` for the project root.
pub fn output_dir(project_root: &Path) -> PathBuf {
    project_root.join(FIBER_DIR).join(OUTPUT_DIR)
}

fn step_diagnosis(step: &StepResult) -> Option<&Value> {
    step.details
        .as_ref()
        .and_then(|details| details.get("diagnosis"))
}

struct DiagnosisGroup<'a> {
    code: &'a str,
    diagnosis: &'a Value,
    steps: Vec<&'a StepResult>,
}

fn diagnosis_groups<'a>(steps: impl Iterator<Item = &'a StepResult>) -> Vec<DiagnosisGroup<'a>> {
    let mut groups: Vec<DiagnosisGroup<'a>> = Vec::new();

    for step in steps {
        let Some(diagnosis) = step_diagnosis(step) else {
            continue;
        };
        let code = diagnosis
            .get("errorCode")
            .and_then(Value::as_str)
            .unwrap_or("FIBER_UNKNOWN_000");

        if let Some(group) = groups.iter_mut().find(|group| group.code == code) {
            group.steps.push(step);
        } else {
            groups.push(DiagnosisGroup {
                code,
                diagnosis,
                steps: vec![step],
            });
        }
    }

    groups
}

fn render_diagnosis_group(
    output: &mut String,
    group: &DiagnosisGroup<'_>,
    remediation_heading: &str,
) {
    let diagnosis = group.diagnosis;
    let sub_category = diagnosis
        .get("subCategory")
        .and_then(Value::as_str)
        .unwrap_or("Unclassified");
    output.push_str(&format!("### `{}` - {}\n\n", group.code, sub_category));

    let affected_steps = group
        .steps
        .iter()
        .map(|step| format!("{} `{}`", step.index, step.action))
        .collect::<Vec<_>>()
        .join(", ");
    output.push_str(&format!("**Affected steps:** {affected_steps}\n\n"));
    output.push_str(&format!(
        "**What happened:** {}\n\n",
        diagnosis_text(
            diagnosis,
            "humanDescription",
            "No human-readable diagnosis was available."
        )
    ));
    output.push_str(&format!(
        "**Why it failed:** {}\n\n",
        diagnosis_text(
            diagnosis,
            "technicalCause",
            "The diagnostic engine could not determine a technical cause."
        )
    ));

    render_string_list(
        output,
        "Likely triggers",
        diagnosis.get("commonTriggers"),
        false,
    );
    render_string_list(
        output,
        remediation_heading,
        diagnosis.get("remediationSteps"),
        true,
    );
}

fn render_expectation_mismatch(output: &mut String, step: &StepResult) {
    output.push_str(&format!(
        "### Step {} `{}` - expectation mismatch\n\n",
        step.index, step.action
    ));
    output.push_str(&format!("**What happened:** {}\n\n", step.message));
    if step.expect == crate::scenario::types::Expectation::Failure {
        output.push_str(
            "**Why it failed:** The action succeeded even though the scenario declared that it should fail. The intended failure precondition was not reproduced.\n\n",
        );
        output.push_str("**What to do next:**\n\n");
        output.push_str(
            "1. Verify the scenario setup actually creates the intended failure condition.\n",
        );
        output.push_str(
            "2. Change `expect: failure` only if success is now the correct behavior.\n\n",
        );
    } else {
        output.push_str(
            "**Why it failed:** No structured diagnosis was attached to this unexpected result.\n\n",
        );
        output.push_str("**What to do next:**\n\n");
        output.push_str("1. Inspect the step message and `logs.json` for the raw result.\n");
        output.push_str("2. Run `fiber doctor \"<raw error text>\" --explain`.\n\n");
    }
}

fn diagnosis_text<'a>(diagnosis: &'a Value, field: &str, fallback: &'a str) -> &'a str {
    diagnosis
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or(fallback)
}

fn render_string_list(output: &mut String, heading: &str, value: Option<&Value>, ordered: bool) {
    let Some(items) = value.and_then(Value::as_array) else {
        return;
    };
    let items = items.iter().filter_map(Value::as_str).collect::<Vec<_>>();
    if items.is_empty() {
        return;
    }

    output.push_str(&format!("**{heading}:**\n\n"));
    for (index, item) in items.iter().enumerate() {
        if ordered {
            output.push_str(&format!("{}. {item}\n", index + 1));
        } else {
            output.push_str(&format!("- {item}\n"));
        }
    }
    output.push('\n');
}

fn step_prediction_summary(step: &StepResult) -> Option<String> {
    if step.action != "predict" {
        return None;
    }
    let details = step.details.as_ref()?;
    if let Some(probability) = details.get("probability").and_then(Value::as_f64) {
        return Some(format!(
            "Step {} `{}` predicted probability {:.2} with confidence `{}`.",
            step.index,
            step.name,
            probability,
            details
                .get("confidence")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ));
    }
    details
        .get("nativeFiber")
        .and_then(|native| native.get("probability"))
        .and_then(Value::as_f64)
        .map(|probability| {
            format!(
                "Step {} `{}` cross-chain comparison kept native probability {:.2}; CCH availability is reported separately.",
                step.index, step.name, probability
            )
        })
}

fn status_label(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Passed => "PASS",
        StepStatus::Failed => "FAIL",
    }
}

fn escape_markdown_table(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::types::{Assertion, AssertionResult, Expectation, StepResult};

    #[test]
    fn markdown_summarizes_steps_without_raw_json() {
        let result = RunResult {
            scenario: "network-smoke".to_string(),
            description: Some("smoke".to_string()),
            passed: true,
            steps: vec![StepResult {
                index: 1,
                name: "node_info".to_string(),
                action: "node_info".to_string(),
                expect: Expectation::Success,
                status: StepStatus::Passed,
                message: "step succeeded as expected".to_string(),
                details: Some(serde_json::json!({ "pubkey": "abc" })),
            }],
            assertions: vec![AssertionResult {
                assertion: Assertion::AllStepsPassed,
                passed: true,
                message: "all steps matched expectations".to_string(),
            }],
        };

        let rendered = Reporter::new(PathBuf::from(".")).markdown(&result);

        assert!(rendered.contains("# Fiber DevKit Scenario Report"));
        assert!(rendered.contains("No corrective action is required"));
        assert!(rendered.contains("| 1 | `node_info` |"));
        assert!(!rendered.contains("\"pubkey\""));
    }

    #[test]
    fn markdown_groups_failures_with_causes_and_all_remediation_steps() {
        let result = RunResult {
            scenario: "network-smoke".to_string(),
            description: Some("smoke".to_string()),
            passed: false,
            steps: vec![
                failed_step(
                    1,
                    "node_info",
                    Expectation::Success,
                    Some(connectivity_diagnosis()),
                ),
                failed_step(
                    2,
                    "graph_nodes",
                    Expectation::Success,
                    Some(connectivity_diagnosis()),
                ),
            ],
            assertions: vec![AssertionResult {
                assertion: Assertion::AllStepsPassed,
                passed: false,
                message: "2 step(s) did not match expectations".to_string(),
            }],
        };

        let rendered = Reporter::new(PathBuf::from(".")).markdown(&result);

        assert!(rendered.contains("## Failure Analysis"));
        assert_eq!(rendered.matches("### `FIBER_CONN_001`").count(), 1);
        assert!(rendered.contains("**Affected steps:** 1 `node_info`, 2 `graph_nodes`"));
        assert!(rendered.contains(
            "**Why it failed:** HTTP transport failed before an FNN JSON-RPC response was returned."
        ));
        assert!(rendered.contains("1. Run `fiber up`."));
        assert!(rendered.contains("2. Run `fiber validate --live`."));
    }

    #[test]
    fn markdown_explains_expected_failures_without_marking_report_failed() {
        let result = RunResult {
            scenario: "low-liquidity".to_string(),
            description: None,
            passed: true,
            steps: vec![StepResult {
                index: 1,
                name: "pay".to_string(),
                action: "pay".to_string(),
                expect: Expectation::Failure,
                status: StepStatus::Passed,
                message: "step failed as expected".to_string(),
                details: Some(serde_json::json!({
                    "diagnosis": {
                        "errorCode": "FIBER_LIQ_001",
                        "subCategory": "InsufficientOutboundCapacity",
                        "humanDescription": "Outbound liquidity is insufficient.",
                        "technicalCause": "Spendable balance is below the payment amount.",
                        "commonTriggers": ["Channel drained"],
                        "remediationSteps": ["Reduce the payment amount."]
                    }
                })),
            }],
            assertions: vec![],
        };

        let rendered = Reporter::new(PathBuf::from(".")).markdown(&result);

        assert!(rendered.contains("**Status:** PASS"));
        assert!(rendered.contains("## Expected Failure Analysis"));
        assert!(rendered.contains("**What to do if unexpected:**"));
        assert!(rendered.contains("1. Reduce the payment amount."));
    }

    #[test]
    fn markdown_explains_success_when_failure_was_expected() {
        let result = RunResult {
            scenario: "expected-failure-mismatch".to_string(),
            description: None,
            passed: false,
            steps: vec![failed_step(1, "pay", Expectation::Failure, None)],
            assertions: vec![],
        };

        let rendered = Reporter::new(PathBuf::from(".")).markdown(&result);

        assert!(rendered.contains("### Step 1 `pay` - expectation mismatch"));
        assert!(rendered.contains("The intended failure precondition was not reproduced."));
        assert!(rendered.contains("Verify the scenario setup"));
    }

    fn failed_step(
        index: usize,
        action: &str,
        expect: Expectation,
        diagnosis: Option<Value>,
    ) -> StepResult {
        StepResult {
            index,
            name: action.to_string(),
            action: action.to_string(),
            expect,
            status: StepStatus::Failed,
            message: if expect == Expectation::Failure {
                "step succeeded but expected failure".to_string()
            } else {
                "step failed: RPC unavailable".to_string()
            },
            details: diagnosis.map(|diagnosis| serde_json::json!({ "diagnosis": diagnosis })),
        }
    }

    fn connectivity_diagnosis() -> Value {
        serde_json::json!({
            "errorCode": "FIBER_CONN_001",
            "subCategory": "NodeRpcUnreachable",
            "humanDescription": "The CLI could not reach a node's JSON-RPC endpoint.",
            "technicalCause": "HTTP transport failed before an FNN JSON-RPC response was returned.",
            "commonTriggers": ["Node offline", "Docker network not running"],
            "remediationSteps": ["Run `fiber up`.", "Run `fiber validate --live`."]
        })
    }
}
