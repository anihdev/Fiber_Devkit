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

        let diagnostics = result
            .steps
            .iter()
            .filter_map(step_diagnosis)
            .collect::<Vec<_>>();
        if !diagnostics.is_empty() {
            output.push_str("## Diagnostics\n\n");
            for (step, diagnosis) in diagnostics {
                output.push_str(&format!(
                    "- Step {} `{}`: `{}` — {}\n",
                    step.index,
                    step.action,
                    diagnosis
                        .get("errorCode")
                        .and_then(Value::as_str)
                        .unwrap_or("FIBER_UNKNOWN_000"),
                    diagnosis
                        .get("humanDescription")
                        .and_then(Value::as_str)
                        .unwrap_or("No diagnosis description available")
                ));
                if let Some(remediation) = diagnosis
                    .get("remediationSteps")
                    .and_then(Value::as_array)
                    .and_then(|steps| steps.first())
                    .and_then(Value::as_str)
                {
                    output.push_str(&format!("  Recommended next step: {remediation}\n"));
                }
            }
            output.push('\n');
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

fn step_diagnosis(step: &StepResult) -> Option<(&StepResult, &Value)> {
    step.details
        .as_ref()
        .and_then(|details| details.get("diagnosis"))
        .map(|diagnosis| (step, diagnosis))
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
        assert!(rendered.contains("| 1 | `node_info` |"));
        assert!(!rendered.contains("\"pubkey\""));
    }
}
