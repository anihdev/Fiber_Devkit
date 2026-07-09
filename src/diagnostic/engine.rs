//! Diagnostic matching and explanation engine.
//! Parses raw errors and prepares diagnostic data for report rendering.

use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::diagnostic::taxonomy::{entry_by_code, TaxonomyEntry, ENTRIES};
use crate::rpc::types::RpcError;
use crate::AppResult;

/// Structured output returned by `fiber doctor` and embedded in failed scenario steps.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosisReport {
    pub error_code: String,
    pub summary: String,
    pub category: String,
    pub sub_category: String,
    pub severity: String,
    pub human_description: String,
    pub technical_cause: String,
    pub common_triggers: Vec<String>,
    pub remediation_steps: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<DiagnosisSource>,
}

/// Location metadata when a diagnosis came from scenario JSONL output.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosisSource {
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_index: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_name: Option<String>,
}

/// Stateless matcher for Fiber taxonomy entries.
#[derive(Default)]
pub struct DiagnosticEngine;

impl DiagnosticEngine {
    /// Creates a diagnostic engine.
    pub fn new() -> Self {
        Self
    }

    /// Matches raw text against taxonomy entries, prioritizing observed Demo 2 failures.
    pub fn parse(&self, raw_error: &str) -> Option<&'static TaxonomyEntry> {
        let normalized = raw_error.to_ascii_lowercase();

        if let Some(entry) = entry_by_code(raw_error.trim()) {
            return Some(entry);
        }

        // Specific liquidity text must win over the broader "failed to build route" string.
        if normalized.contains("insufficient balance")
            || normalized.contains("max outbound liquidity")
            || normalized.contains("insufficient outbound")
        {
            return entry_by_code("FIBER_LIQ_001");
        }

        if normalized.contains("error sending request")
            || normalized.contains("connection refused")
            || normalized.contains("tcp connect error")
            || normalized.contains("deadline has elapsed")
        {
            return entry_by_code("FIBER_CONN_001");
        }

        if normalized.contains("closed before becoming ready") {
            return entry_by_code("FIBER_CHAN_003");
        }

        if normalized.contains("did not become ready") {
            return entry_by_code("FIBER_CHAN_002");
        }

        if let Some(entry) = parse_cch(&normalized) {
            return Some(entry);
        }

        ENTRIES.iter().find(|entry| {
            entry
                .matchers
                .iter()
                .any(|matcher| normalized.contains(matcher))
        })
    }

    /// Converts a raw error string into a structured report.
    pub fn diagnose_text(&self, raw_error: &str) -> DiagnosisReport {
        let observed_error = if raw_error.trim().is_empty() {
            None
        } else {
            Some(raw_error.trim().to_string())
        };
        match self.parse(raw_error) {
            Some(entry) => report_from_entry(self, entry, observed_error, None),
            None => unknown_report(observed_error, None),
        }
    }

    /// Converts an RPC-layer error into a structured report.
    pub fn diagnose_rpc_error(&self, error: &RpcError) -> DiagnosisReport {
        let raw_error = match error {
            RpcError::Rpc {
                code,
                message,
                data,
            } => {
                if let Some(data) = data {
                    format!("RPC error {code}: {message}; data={data}")
                } else {
                    format!("RPC error {code}: {message}")
                }
            }
            _ => error.message(),
        };
        self.diagnose_text(&raw_error)
    }

    /// Parses JSONL output from `fiber run` and diagnoses every observed failed RPC step.
    pub fn parse_log_file(&self, path: &Path) -> AppResult<Vec<DiagnosisReport>> {
        let raw = fs::read_to_string(path)?;
        let mut reports = Vec::new();

        for (line_index, line) in raw.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
                let mut report = self.diagnose_text(trimmed);
                if report.error_code != "FIBER_UNKNOWN_000" {
                    report.source = Some(DiagnosisSource {
                        line: line_index + 1,
                        step_index: None,
                        action: None,
                        step_name: None,
                    });
                    reports.push(report);
                }
                continue;
            };

            if let Some(raw_error) = observed_error_from_step(&value) {
                let source = DiagnosisSource {
                    line: line_index + 1,
                    step_index: value.get("index").and_then(Value::as_u64),
                    action: value
                        .get("action")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    step_name: value
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                };
                let mut report = self.diagnose_text(&raw_error);
                report.source = Some(source);
                reports.push(report);
            }
        }

        Ok(reports)
    }

    /// Returns a compact one-line description for a taxonomy entry.
    pub fn humanize(&self, entry: &TaxonomyEntry) -> String {
        format!("{}: {}", entry.code, entry.description)
    }

    /// Renders a readable multi-line explanation for one diagnosis.
    pub fn explain(&self, report: &DiagnosisReport) -> String {
        let template = entry_by_code(&report.error_code)
            .map(|entry| entry.explain_template)
            .unwrap_or("The error did not match a known Fiber taxonomy entry.");

        let mut output = format!(
            "{} — {}\n\n{}\n",
            report.error_code, report.human_description, template
        );

        if let Some(observed) = &report.observed_error {
            output.push_str("\nObserved error:\n");
            output.push_str(observed);
            output.push('\n');
        }

        if !report.remediation_steps.is_empty() {
            output.push_str("\nPossible fixes:\n");
            for step in &report.remediation_steps {
                output.push_str("- ");
                output.push_str(step);
                output.push('\n');
            }
        }

        output
    }
}

fn parse_cch(normalized: &str) -> Option<&'static TaxonomyEntry> {
    let has_cch_context = normalized.contains("cch")
        || normalized.contains("btc_pay_req")
        || normalized.contains("fiber_pay_req")
        || normalized.contains("outgoinginflight");

    if !has_cch_context {
        return None;
    }

    // CCH-specific classes are ordered before gateway availability so a generic
    // gateway mention does not hide the actionable order-state or invoice cause.
    if normalized.contains("unsupported cch asset")
        || normalized.contains("wrapped_btc_type_script")
        || normalized.contains("wrapped btc")
        || (normalized.contains("cch")
            && (normalized.contains("unsupported asset")
                || normalized.contains("non-btc")
                || normalized.contains("not btc")))
    {
        return entry_by_code("FIBER_CCH_002");
    }

    if normalized.contains("pay_req")
        || normalized.contains("btc_pay_req")
        || normalized.contains("fiber_pay_req")
        || normalized.contains("invoice")
    {
        return entry_by_code("FIBER_CCH_003");
    }

    if normalized.contains("incoming tlc")
        || normalized.contains("incomingtlc")
        || normalized.contains("pending expired")
        || (normalized.contains("pending") && normalized.contains("expired"))
    {
        return entry_by_code("FIBER_CCH_004");
    }

    if normalized.contains("outgoinginflight")
        || normalized.contains("outgoing in flight")
        || normalized.contains("outgoing payment")
    {
        return entry_by_code("FIBER_CCH_005");
    }

    if normalized.contains("gateway unavailable")
        || normalized.contains("no cch gateway")
        || normalized.contains("cch gateway unavailable")
        || normalized.contains("gateway peer")
        || normalized.contains("gateway not configured")
    {
        return entry_by_code("FIBER_CCH_001");
    }

    None
}

fn observed_error_from_step(value: &Value) -> Option<String> {
    let details = value.get("details")?;
    let has_error_details = details.get("kind").is_some()
        || details.get("message").is_some()
        || value
            .get("message")
            .and_then(Value::as_str)
            .is_some_and(|message| message.contains("failed"));

    if !has_error_details {
        return None;
    }

    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let detail_message = details
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| details.to_string());

    if message.is_empty() {
        Some(detail_message)
    } else {
        Some(format!("{message}; {detail_message}"))
    }
}

fn report_from_entry(
    engine: &DiagnosticEngine,
    entry: &TaxonomyEntry,
    observed_error: Option<String>,
    source: Option<DiagnosisSource>,
) -> DiagnosisReport {
    DiagnosisReport {
        error_code: entry.code.to_string(),
        summary: engine.humanize(entry),
        category: entry.category.to_string(),
        sub_category: entry.sub_category.to_string(),
        severity: entry.severity.to_string(),
        human_description: entry.description.to_string(),
        technical_cause: entry.technical_cause.to_string(),
        common_triggers: entry
            .common_triggers
            .iter()
            .map(|item| item.to_string())
            .collect(),
        remediation_steps: entry
            .remediation_steps
            .iter()
            .map(|item| item.to_string())
            .collect(),
        observed_error,
        source,
    }
}

fn unknown_report(
    observed_error: Option<String>,
    source: Option<DiagnosisSource>,
) -> DiagnosisReport {
    DiagnosisReport {
        error_code: "FIBER_UNKNOWN_000".to_string(),
        summary: "FIBER_UNKNOWN_000: The error did not match a known Demo 3 taxonomy entry."
            .to_string(),
        category: "Unknown".to_string(),
        sub_category: "Unclassified".to_string(),
        severity: "Low".to_string(),
        human_description: "The error did not match a known Demo 3 taxonomy entry.".to_string(),
        technical_cause: "No diagnostic matcher recognized the observed text.".to_string(),
        common_triggers: vec![
            "New FNN error shape".to_string(),
            "Non-payment CLI failure".to_string(),
        ],
        remediation_steps: vec![
            "Inspect the raw error output.".to_string(),
            "Add a new taxonomy entry if this failure is reproducible.".to_string(),
        ],
        observed_error,
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnoses_observed_low_liquidity_error() {
        let engine = DiagnosticEngine::new();
        let report = engine.diagnose_text(
            "Send payment error: Failed to build route, Insufficient balance: max outbound liquidity 10000000000 is insufficient, required amount: 10100000000",
        );

        assert_eq!(report.error_code, "FIBER_LIQ_001");
        assert_eq!(report.category, "Liquidity");
    }

    #[test]
    fn diagnoses_observed_peer_offline_error() {
        let engine = DiagnosticEngine::new();
        let report =
            engine.diagnose_text("error sending request for url (http://127.0.0.1:65530/)");

        assert_eq!(report.error_code, "FIBER_CONN_001");
        assert_eq!(report.category, "Connectivity");
    }

    #[test]
    fn parses_scenario_jsonl_failures() {
        let path = std::env::temp_dir().join(format!(
            "fiber-doctor-jsonl-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(
            &path,
            r#"{"index":3,"name":"pay","action":"pay","status":"failed","message":"step failed: Send payment error: Failed to build route, Insufficient balance: max outbound liquidity 10000000000 is insufficient, required amount: 10100000000","details":{"kind":"rpc","message":"Send payment error: Failed to build route, Insufficient balance: max outbound liquidity 10000000000 is insufficient, required amount: 10100000000"}}"#,
        )
        .unwrap();

        let reports = DiagnosticEngine::new().parse_log_file(&path).unwrap();
        fs::remove_file(path).unwrap();

        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].error_code, "FIBER_LIQ_001");
        assert_eq!(reports[0].source.as_ref().unwrap().step_index, Some(3));
    }

    #[test]
    fn cch_pay_req_error_routes_to_invoice_validation() {
        let engine = DiagnosticEngine::new();
        let report =
            engine.diagnose_text("CCH gateway rejected btc_pay_req: invoice amount mismatch");

        assert_eq!(report.error_code, "FIBER_CCH_003");
    }

    #[test]
    fn cch_unsupported_asset_routes_to_asset_error() {
        let engine = DiagnosticEngine::new();
        let report = engine.diagnose_text("CCH gateway rejected unsupported asset: non-BTC UDT");

        assert_eq!(report.error_code, "FIBER_CCH_002");
    }

    #[test]
    fn cch_pending_expiry_routes_to_incoming_tlc_timeout() {
        let engine = DiagnosticEngine::new();
        let report =
            engine.diagnose_text("CCH order stayed Pending and expired before incoming TLC");

        assert_eq!(report.error_code, "FIBER_CCH_004");
    }

    #[test]
    fn cch_outgoing_in_flight_routes_to_outgoing_payment_failure() {
        let engine = DiagnosticEngine::new();
        let report = engine
            .diagnose_text("CCH order entered OutgoingInFlight and failed during outgoing payment");

        assert_eq!(report.error_code, "FIBER_CCH_005");
    }

    #[test]
    fn cch_gateway_text_does_not_shadow_specific_invoice_error() {
        let engine = DiagnosticEngine::new();
        let report =
            engine.diagnose_text("CCH gateway returned malformed fiber_pay_req invoice data");

        assert_eq!(report.error_code, "FIBER_CCH_003");
    }

    #[test]
    fn cch_gateway_unavailable_still_matches_gateway_error() {
        let engine = DiagnosticEngine::new();
        let report = engine.diagnose_text("No CCH gateway peer configured for bridge order");

        assert_eq!(report.error_code, "FIBER_CCH_001");
    }
}
