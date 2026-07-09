//! MVP FNN node template profiles.
//! Defines hub and leaf runtime knobs used during config generation.

use crate::config::NodeTemplate;

/// Runtime profile values copied into generated FNN YAML config.
#[derive(Clone, Copy)]
pub struct TemplateProfile {
    pub max_inbound_peers: usize,
    pub min_outbound_peers: usize,
    pub auto_accept_channel_ckb_funding_amount: u64,
    pub open_channel_auto_accept_min_ckb_funding_amount: u64,
    pub tlc_fee_proportional_millionths: u128,
}

/// Returns the built-in profile for an MVP node template.
pub fn profile(template: NodeTemplate) -> TemplateProfile {
    match template {
        NodeTemplate::Hub => TemplateProfile {
            max_inbound_peers: 32,
            min_outbound_peers: 0,
            // FNN requires the accepting side to reserve CKB even for private one-way channels.
            auto_accept_channel_ckb_funding_amount: 9_900_000_000,
            open_channel_auto_accept_min_ckb_funding_amount: 100_000_000,
            tlc_fee_proportional_millionths: 1_000,
        },
        NodeTemplate::Leaf => TemplateProfile {
            max_inbound_peers: 8,
            min_outbound_peers: 0,
            // FNN requires the accepting side to reserve CKB even for private one-way channels.
            auto_accept_channel_ckb_funding_amount: 9_900_000_000,
            open_channel_auto_accept_min_ckb_funding_amount: 100_000_000,
            tlc_fee_proportional_millionths: 1_000,
        },
    }
}
