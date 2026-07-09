//! OTel-compatible trace event rendering for scenario runs.
//! Defines the span schema used for `trace.json` output.

use serde::Serialize;
use serde_json::Value;

use crate::scenario::types::{RunResult, StepResult, StepStatus};

/// Minimal OTel-compatible trace document written to `.fiber/output/trace.json`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceDocument {
    pub resource_spans: Vec<ResourceSpans>,
}

/// Resource-scoped span collection for one scenario run.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSpans {
    pub resource: TraceResource,
    pub scope_spans: Vec<ScopeSpans>,
}

/// Static service metadata attached to every generated trace.
#[derive(Debug, Clone, Serialize)]
pub struct TraceResource {
    pub attributes: Vec<TraceAttribute>,
}

/// Scope metadata and spans for the DevKit scenario runner.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeSpans {
    pub scope: TraceScope,
    pub spans: Vec<TraceSpan>,
}

/// Instrumentation scope metadata.
#[derive(Debug, Clone, Serialize)]
pub struct TraceScope {
    pub name: String,
}

/// One scenario setup item or step rendered as a span.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceSpan {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: String,
    pub name: String,
    pub kind: String,
    pub status: TraceStatus,
    pub attributes: Vec<TraceAttribute>,
}

/// OTel-style span status.
#[derive(Debug, Clone, Serialize)]
pub struct TraceStatus {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// String attribute used by the simple trace document.
#[derive(Debug, Clone, Serialize)]
pub struct TraceAttribute {
    pub key: String,
    pub value: TraceValue,
}

/// String-only attribute value, matching the OTel JSON field shape.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceValue {
    pub string_value: String,
}

/// Converts a scenario run into a deterministic trace document.
pub fn trace_from_run(result: &RunResult) -> TraceDocument {
    let trace_id = stable_trace_id(&result.scenario);
    let spans = result
        .steps
        .iter()
        .map(|step| span_from_step(step, &trace_id))
        .collect();

    TraceDocument {
        resource_spans: vec![ResourceSpans {
            resource: TraceResource {
                attributes: vec![
                    attr("service.name", "fiber-devkit"),
                    attr("fiber.scenario", &result.scenario),
                    attr("fiber.run.passed", result.passed.to_string()),
                ],
            },
            scope_spans: vec![ScopeSpans {
                scope: TraceScope {
                    name: "fiber-devkit.scenario".to_string(),
                },
                spans,
            }],
        }],
    }
}

fn span_from_step(step: &StepResult, trace_id: &str) -> TraceSpan {
    let mut attributes = vec![
        attr("fiber.step.index", step.index.to_string()),
        attr("fiber.step.action", &step.action),
        attr(
            "fiber.step.expect",
            format!("{:?}", step.expect).to_lowercase(),
        ),
        attr(
            "fiber.step.status",
            format!("{:?}", step.status).to_lowercase(),
        ),
    ];
    if let Some(code) = diagnosis_code(step) {
        attributes.push(attr("fiber.diagnosis.code", code));
    }
    if let Some(probability) = prediction_probability(step) {
        attributes.push(attr("fiber.prediction.probability", probability));
    }

    TraceSpan {
        trace_id: trace_id.to_string(),
        span_id: format!("{:016x}", step.index),
        parent_span_id: String::new(),
        name: step.name.clone(),
        kind: "SPAN_KIND_INTERNAL".to_string(),
        status: TraceStatus {
            code: if step.status == StepStatus::Passed {
                "STATUS_CODE_OK".to_string()
            } else {
                "STATUS_CODE_ERROR".to_string()
            },
            message: (step.status == StepStatus::Failed).then(|| step.message.clone()),
        },
        attributes,
    }
}

fn diagnosis_code(step: &StepResult) -> Option<&str> {
    step.details
        .as_ref()
        .and_then(|details| details.get("diagnosis"))
        .and_then(|diagnosis| diagnosis.get("errorCode"))
        .and_then(Value::as_str)
}

fn prediction_probability(step: &StepResult) -> Option<String> {
    let details = step.details.as_ref()?;
    details
        .get("probability")
        .or_else(|| {
            details
                .get("nativeFiber")
                .and_then(|native| native.get("probability"))
        })
        .and_then(Value::as_f64)
        .map(|value| format!("{value:.2}"))
}

fn attr(key: impl Into<String>, value: impl ToString) -> TraceAttribute {
    TraceAttribute {
        key: key.into(),
        value: TraceValue {
            string_value: value.to_string(),
        },
    }
}

fn stable_trace_id(seed: &str) -> String {
    let mut hash: u128 = 0x1234_5678_9abc_def0_1020_3040_5060_7080;
    for byte in seed.as_bytes() {
        hash = hash.rotate_left(5) ^ u128::from(*byte);
    }
    format!("{hash:032x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::types::{Expectation, StepResult};

    #[test]
    fn trace_contains_one_span_per_step() {
        let result = RunResult {
            scenario: "smoke".to_string(),
            description: None,
            passed: true,
            steps: vec![StepResult {
                index: 1,
                name: "node_info".to_string(),
                action: "node_info".to_string(),
                expect: Expectation::Success,
                status: StepStatus::Passed,
                message: "ok".to_string(),
                details: None,
            }],
            assertions: Vec::new(),
        };

        let trace = trace_from_run(&result);

        assert_eq!(trace.resource_spans[0].scope_spans[0].spans.len(), 1);
        assert_eq!(
            trace.resource_spans[0].scope_spans[0].spans[0].status.code,
            "STATUS_CODE_OK"
        );
    }
}
