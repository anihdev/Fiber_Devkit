//! Cross-chain comparison data for route prediction.
//! Owns the honest CCH bridge availability statement; it deliberately does not
//! model CCH as a routed path with probability or hop count.

use serde::Serialize;

use crate::route::analyzer::PaymentPrediction;

/// Combined output for `fiber predict --cross-chain`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteComparison {
    pub native_fiber: PaymentPrediction,
    pub cch_bridged: CchPathResult,
    pub recommendation: RouteChoice,
    pub reason: String,
}

/// High-level recommendation after comparing native Fiber and CCH applicability.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteChoice {
    Native,
    CchBridged,
    Unavailable,
}

/// CCH bridge statement shown beside native route prediction.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CchPathResult {
    pub available: bool,
    pub mechanism: &'static str,
    pub supported_assets: Vec<&'static str>,
    pub operator_configured: bool,
    pub reason: String,
    pub note: String,
}

impl CchPathResult {
    /// Returns the Demo 4 fallback when no live CCH gateway/order probe is configured.
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            available: false,
            mechanism: "swap",
            supported_assets: vec!["BTC <-> wrapped-BTC (1:1, fixed rate)"],
            operator_configured: false,
            reason: reason.into(),
            note: "CCH bridges BTC and wrapped-BTC only; it is not a multi-asset router and has no path/probability concept comparable to native Fiber routing.".to_string(),
        }
    }
}
