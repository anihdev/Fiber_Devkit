//! YAML parser for Fiber DevKit scenarios.
//! Owns schema validation and friendly parse errors; it does not perform live
//! network checks or execute any RPC calls.

use std::fs;
use std::path::Path;

use crate::scenario::types::{Assertion, Scenario};
use crate::{app_error, AppResult};

/// Parser for the MVP scenario format documented in `SCENARIO_FORMAT.md`.
pub struct ScenarioParser;

impl ScenarioParser {
    /// Reads a scenario YAML file and validates cross-field invariants.
    pub fn parse(path: &Path) -> AppResult<Scenario> {
        let raw = fs::read_to_string(path)?;
        let mut scenario: Scenario = serde_yaml::from_str(&raw)?;
        scenario.source_path = Some(path.to_path_buf());
        validate(&scenario)?;
        Ok(scenario)
    }
}

fn validate(scenario: &Scenario) -> AppResult<()> {
    if scenario.name.trim().is_empty() {
        return Err(app_error("scenario name must not be empty"));
    }
    if scenario.nodes.is_empty() {
        return Err(app_error("scenario must define at least one node alias"));
    }
    if scenario.steps.is_empty() {
        return Err(app_error("scenario must define at least one step"));
    }

    for (alias, node) in &scenario.nodes {
        if alias.trim().is_empty() {
            return Err(app_error("scenario node alias must not be empty"));
        }
        match (&node.node, &node.endpoint) {
            (Some(_), None) | (None, Some(_)) => {}
            (Some(_), Some(_)) => {
                return Err(app_error(format!(
                    "node alias `{alias}` must not define both node and endpoint"
                )));
            }
            (None, None) => {
                return Err(app_error(format!(
                    "node alias `{alias}` must define node or endpoint"
                )));
            }
        }
    }

    for channel in &scenario.channels {
        ensure_alias(scenario, &channel.from)?;
        ensure_alias(scenario, &channel.to)?;
        parse_ckb_amount(&channel.capacity)?;
    }

    for step in &scenario.steps {
        step.validate(scenario)?;
    }

    for assertion in &scenario.assertions {
        match assertion {
            Assertion::AllStepsPassed => {}
        }
    }

    Ok(())
}

/// Converts strings like `1.5 CKB` into shannons.
pub fn parse_ckb_amount(amount: &str) -> AppResult<u128> {
    let trimmed = amount.trim();
    let numeric = trimmed
        .strip_suffix("CKB")
        .or_else(|| trimmed.strip_suffix("ckb"))
        .ok_or_else(|| app_error(format!("amount `{amount}` must use the CKB suffix")))?;
    let numeric = numeric.trim();
    if numeric.is_empty() {
        return Err(app_error(format!("amount `{amount}` is missing a value")));
    }

    let parts: Vec<_> = numeric.split('.').collect();
    if parts.len() > 2 {
        return Err(app_error(format!(
            "amount `{amount}` is not a valid decimal"
        )));
    }

    let whole = parts[0]
        .parse::<u128>()
        .map_err(|_| app_error(format!("amount `{amount}` is not a valid number")))?;
    let fractional = if parts.len() == 2 {
        let fraction = parts[1];
        if fraction.len() > 8 || !fraction.chars().all(|ch| ch.is_ascii_digit()) {
            return Err(app_error(format!(
                "amount `{amount}` supports at most 8 decimal places"
            )));
        }
        let padded = format!("{fraction:0<8}");
        padded
            .parse::<u128>()
            .map_err(|_| app_error(format!("amount `{amount}` is not a valid number")))?
    } else {
        0
    };

    whole
        .checked_mul(100_000_000)
        .and_then(|value| value.checked_add(fractional))
        .ok_or_else(|| app_error(format!("amount `{amount}` is too large")))
}

fn ensure_alias(scenario: &Scenario, alias: &str) -> AppResult<()> {
    if scenario.nodes.contains_key(alias) {
        Ok(())
    } else {
        Err(app_error(format!("unknown node alias `{alias}`")))
    }
}

trait ValidateStep {
    fn validate(&self, scenario: &Scenario) -> AppResult<()>;
}

impl ValidateStep for crate::scenario::types::Step {
    fn validate(&self, scenario: &Scenario) -> AppResult<()> {
        match self {
            Self::NodeInfo { node, .. }
            | Self::ListChannels { node, .. }
            | Self::GraphNodes { node, .. }
            | Self::GraphChannels { node, .. } => ensure_alias(scenario, node),
            Self::OpenChannel {
                from, to, capacity, ..
            } => {
                ensure_alias(scenario, from)?;
                ensure_alias(scenario, to)?;
                parse_ckb_amount(capacity)?;
                Ok(())
            }
            Self::Pay {
                from,
                to,
                amount,
                max_fee,
                ..
            } => {
                ensure_alias(scenario, from)?;
                ensure_alias(scenario, to)?;
                parse_ckb_amount(amount)?;
                if let Some(max_fee) = max_fee {
                    parse_ckb_amount(max_fee)?;
                }
                Ok(())
            }
            Self::Predict {
                from,
                to,
                amount,
                expect_probability_above,
                expect_probability_below,
                ..
            } => {
                ensure_alias(scenario, from)?;
                ensure_alias(scenario, to)?;
                parse_ckb_amount(amount)?;
                validate_probability_bound(*expect_probability_above)?;
                validate_probability_bound(*expect_probability_below)?;
                Ok(())
            }
        }
    }
}

fn validate_probability_bound(value: Option<f64>) -> AppResult<()> {
    if let Some(value) = value {
        if !(0.0..=1.0).contains(&value) {
            return Err(app_error(
                "prediction probability expectations must be between 0.0 and 1.0",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ckb_amounts_to_shannons() {
        assert_eq!(parse_ckb_amount("1 CKB").unwrap(), 100_000_000);
        assert_eq!(parse_ckb_amount("0.5 CKB").unwrap(), 50_000_000);
        assert_eq!(parse_ckb_amount("1.00000001 CKB").unwrap(), 100_000_001);
    }

    #[test]
    fn rejects_unknown_aliases() {
        let yaml = r#"
name: bad
nodes:
  alice: { node: node-1 }
steps:
  - action: node_info
    node: bob
"#;
        let scenario: Scenario = serde_yaml::from_str(yaml).unwrap();
        assert!(validate(&scenario).is_err());
    }

    #[test]
    fn parses_predict_step_probability_bounds() {
        let yaml = r#"
name: predict
nodes:
  alice: { node: node-1 }
  bob: { node: node-2 }
steps:
  - action: predict
    from: alice
    to: bob
    amount: "1 CKB"
    expect_probability_above: 0.85
"#;
        let scenario: Scenario = serde_yaml::from_str(yaml).unwrap();
        assert!(validate(&scenario).is_ok());
    }
}
