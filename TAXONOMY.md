# Fiber Payment Failure Taxonomy

This document defines the Demo 3 diagnostic taxonomy implemented by `fiber doctor`.
It is grounded in real FNN v0.9.0-rc5 scenario output where available and keeps future
codes additive: new entries should not rename or repurpose existing codes.

This taxonomy is maintained as a standalone specification. It is designed to be reusable by any Fiber tooling that needs to classify payment failures, not only Fiber DevKit's own fiber doctor implementation.

## Doctor Input Scope

Demo 3 accepts scenario JSONL log files emitted by `fiber run`, known taxonomy codes,
and raw error text. It does not perform payment-hash lookup yet because DevKit does not
persist run history or node/RPC context for historical payment queries.

## Observed Demo 2 Failures

`scenarios/low-liquidity.yaml` produced this FNN payment failure:

```text
Send payment error: Failed to build route, Insufficient balance: max outbound liquidity 10000000000 is insufficient, required amount: 10100000000
```

This maps to `FIBER_LIQ_001`.

`scenarios/peer-offline.yaml` produced this unreachable endpoint failure:

```text
error sending request for url (http://127.0.0.1:65530/)
```

This maps to `FIBER_CONN_001`.

## Entry Format

Each entry has:

- `code`: stable machine-readable taxonomy code
- `category`: top-level failure family
- `subCategory`: narrower failure class
- `severity`: `Critical`, `High`, `Medium`, or `Low`
- `description`: one-line human explanation
- `technicalCause`: implementation-level cause
- `commonTriggers`: common scenario or runtime triggers
- `remediationSteps`: concrete fixes a developer can try
- `explainTemplate`: narrative text printed by `fiber doctor --explain`

## Route Intelligence

Demo 4 implements the `fiber predict` route scoring model documented here. The doctor
command does not run prediction itself, but liquidity and routing diagnoses use the same
language so prediction output and taxonomy remediation stay aligned.

```text
CapacityScore = min(outbound_capacity / required_amount, 1.0)  weight 0.40
HopPenalty    = 1.0 - (hop_count - 1) * 0.05                  weight 0.20
ChannelHealth = active_time_ratio of each hop's channel       weight 0.20
FeeCost       = 1.0 - (total_fee / fee_tolerance)             weight 0.20

RouteScore = (0.40 * CapacityScore)
           + (0.20 * HopPenalty)
           + (0.20 * ChannelHealth)
           + (0.20 * FeeCost)
```

If no path has usable outbound capacity, route probability is `0.0` and the diagnostic
falls under `FIBER_LIQ_001` or `FIBER_ROUTE_001`, depending on whether a channel path
exists but lacks liquidity, or no path exists at all.

Prediction prefers directional local balances from `list_channels`. When those balances
are unavailable and prediction falls back to `graph_channels`, graph capacity is treated
as topology-only, not proven directional liquidity. Those routes are labelled
`graph_topology`, capped at low confidence, and reported with `FIBER_ROUTE_002`.

## ROUTE

## FIBER_ROUTE_001 : No Route To Target
**Category:** Route
**Severity:** High
**Description:** FNN could not find any route from the sender to the target node.
**Technical cause:** The route builder had no graph path that connected source and destination.
**Common triggers:** Missing peer connection; graph gossip not propagated; target pubkey unknown.
**Remediation:** Connect the participating peers, run `fiber run scenarios/network-smoke.yaml`, and retry after graph nodes are visible.

## FIBER_ROUTE_002 : Stale Graph
**Category:** Route
**Severity:** Medium
**Description:** The local graph view is stale or incomplete.
**Technical cause:** `graph_nodes` or `graph_channels` does not yet reflect recently opened or closed channels.
**Common triggers:** Running a payment immediately after channel setup; node restart before gossip catches up.
**Remediation:** Query `graph_nodes` and `graph_channels`, wait for gossip propagation, or use direct `list_channels` checks for local scenarios.

## FIBER_ROUTE_003 : Route Build Failed
**Category:** Route
**Severity:** High
**Description:** FNN entered route construction but could not produce a valid candidate route.
**Technical cause:** The route builder rejected all candidate paths due route, channel, fee, or liquidity constraints.
**Common triggers:** Mixed route and liquidity failures; disabled channel; fee policy conflict.
**Remediation:** Inspect the nested error text, then apply the more specific `FIBER_LIQ`, `FIBER_CHAN`, or `FIBER_FEE` remediation.

## LIQ

## FIBER_LIQ_001 : Insufficient Outbound Capacity
**Category:** Liquidity
**Severity:** High
**Description:** A sender or forwarding hop does not have enough outbound liquidity for the payment amount.
**Technical cause:** FNN reported max outbound liquidity below the required amount.
**Common triggers:** Payment amount exceeds `local_balance`; channel reserves reduce spendable capacity; one-way test channel is too small.
**Remediation:** Reduce the payment amount, open a larger channel, or fund/rebalance the outbound side before retrying.

## FIBER_LIQ_002 : Directional Liquidity Mismatch
**Category:** Liquidity
**Severity:** High
**Description:** Total channel capacity exists, but it is on the wrong side for this payment direction.
**Technical cause:** The channel's remote/local balance distribution cannot forward in the requested direction.
**Common triggers:** One-way channel used in reverse; previous payments drained the sender side.
**Remediation:** Send in the supported direction, rebalance, or open a new channel from the paying side.

## FIBER_LIQ_003 : Channel Reserve Exceeded
**Category:** Liquidity
**Severity:** Medium
**Description:** The apparent channel capacity is reduced by CKB reserve and shutdown requirements.
**Technical cause:** FNN withholds part of `funding_amount` for commitment and shutdown safety.
**Common triggers:** Expecting a `199 CKB` channel to carry the full `199 CKB`; auto-accept reserve on the accepting peer.
**Remediation:** Size test channels above the intended payment amount and account for the observed reserve documented in `SCENARIO_FORMAT.md`.

## FIBER_LIQ_004 : Inbound Capacity Unavailable
**Category:** Liquidity
**Severity:** Medium
**Description:** The receiver or final hop cannot accept the requested amount.
**Technical cause:** The destination side lacks inbound capacity on the selected path.
**Common triggers:** Receiver-only test topology; depleted remote balance.
**Remediation:** Open capacity toward the receiver or use a route with available inbound liquidity.

## CONN

## FIBER_CONN_001 : Node RPC Unreachable
**Category:** Connectivity
**Severity:** High
**Description:** The CLI could not reach a node's JSON-RPC endpoint.
**Technical cause:** HTTP transport failed before an FNN JSON-RPC response was returned.
**Common triggers:** Node offline; wrong port; endpoint points at `127.0.0.1:65530`; Docker network not running.
**Remediation:** Run `fiber up`, verify the endpoint port, then run `fiber validate --live`.

## FIBER_CONN_002 : Peer Disconnected
**Category:** Connectivity
**Severity:** Medium
**Description:** The FNN node is reachable, but the required Fiber peer is not connected.
**Technical cause:** P2P peer count or graph data does not include the expected counterparty.
**Common triggers:** Hub-spoke connection failed during startup; peer container restarted.
**Remediation:** Restart with `fiber down && fiber up`, then confirm peer counts in `node_info`.

## FIBER_CONN_003 : RPC Authentication Required
**Category:** Connectivity
**Severity:** Medium
**Description:** The RPC endpoint rejected the request because authentication is required.
**Technical cause:** The node is configured as a public RPC address with Biscuit authorization enabled.
**Common triggers:** Using `0.0.0.0` or a public address without a token; non-localhost node config.
**Remediation:** Use localhost DevKit endpoints or provide a valid Biscuit token for public nodes.

## CHAN

## FIBER_CHAN_001 : Channel Not Ready
**Category:** Channel
**Severity:** High
**Description:** A scenario attempted to use a channel before FNN reported `ChannelReady`.
**Technical cause:** The channel is still opening, pending funding, or waiting for CKB confirmation.
**Common triggers:** Payment starts immediately after `open_channel`; CKB RPC is slow.
**Remediation:** Wait for `list_channels` to show `ChannelReady` before sending payments.

## FIBER_CHAN_002 : Channel Opening Timed Out
**Category:** Channel
**Severity:** High
**Description:** A channel did not become ready inside the scenario setup timeout.
**Technical cause:** Funding negotiation or CKB confirmation failed to complete before the runner deadline.
**Common triggers:** Unfunded generated keys; CKB RPC timeout; peer rejected the funding proposal.
**Remediation:** Run `pnpm balances:nodes`, `pnpm fund:nodes`, then retry from `fiber reset`.

## FIBER_CHAN_003 : Channel Closed Before Ready
**Category:** Channel
**Severity:** High
**Description:** FNN closed the channel during setup before it could become usable.
**Technical cause:** `list_channels` reported `Closed` with a failure detail while the runner waited for readiness.
**Common triggers:** Same-lock funding source; insufficient funding cells; peer rejects public one-way channel.
**Remediation:** Use distinct funded generated node keys and keep one-way channels private.

## FEE

## FIBER_FEE_001 : Fee Exceeds Maximum
**Category:** Fee
**Severity:** Medium
**Description:** Payment fees exceeded the caller's configured maximum fee.
**Technical cause:** `max_fee_amount` or fee tolerance is lower than the route's required fee.
**Common triggers:** Very low `max_fee`; multi-hop route with nonzero proportional fees.
**Remediation:** Increase the max fee or choose a lower-fee route.

## FIBER_FEE_002 : Fee Policy Mismatch
**Category:** Fee
**Severity:** Medium
**Description:** A hop's fee policy makes the otherwise valid route unusable.
**Technical cause:** Fee policy or proportional millionths conflict with route constraints.
**Common triggers:** Custom node template with strict fees; stale fee policy in graph.
**Remediation:** Inspect channel policies and retry after gossip updates.

## ASSET

## FIBER_ASSET_001 : Unsupported Asset
**Category:** Asset
**Severity:** High
**Description:** The requested asset is not supported by one or more nodes on the path.
**Technical cause:** The route lacks compatible UDT or CKB funding support.
**Common triggers:** Payment uses an asset absent from `udt_whitelist`; CCH requested for non-BTC asset.
**Remediation:** Use CKB for MVP scenarios or configure all nodes with the same asset script.

## FIBER_ASSET_002 : Asset Type Mismatch
**Category:** Asset
**Severity:** High
**Description:** Route candidates disagree on the asset type script.
**Technical cause:** Channel funding UDT type does not match the requested payment asset.
**Common triggers:** Mixed CKB and UDT channels; wrong wrapped BTC type script.
**Remediation:** Recreate channels with matching asset configuration.

## HTLC

## FIBER_HTLC_001 : TLC Timeout
**Category:** HTLC
**Severity:** High
**Description:** A time-locked conditional payment expired before settlement.
**Technical cause:** The TLC reached its expiry delta without a successful preimage flow.
**Common triggers:** Slow or disconnected hop; route too long for timeout settings.
**Remediation:** Increase timeout, use a shorter route, or restore connectivity before retrying.

## FIBER_HTLC_002 : Payment Failed Terminal
**Category:** HTLC
**Severity:** High
**Description:** FNN reported the payment as terminally failed without a more specific route or liquidity reason.
**Technical cause:** The payment state reached `Failed` and exposed `failed_error`.
**Common triggers:** Downstream rejection; malformed payment parameters; implementation-specific failure.
**Remediation:** Inspect `failed_error`, then retry only after the more specific cause is corrected.

## JAM

## FIBER_JAM_001 : Rate Limited Or Jammed
**Category:** Jamming
**Severity:** Medium
**Description:** A node or channel appears to reject traffic due rate limits or jamming controls.
**Technical cause:** Repeated payment attempts or in-flight TLCs exhaust node/channel policy limits.
**Common triggers:** Stress scenario; many failed attempts; pending TLC buildup.
**Remediation:** Wait for pending TLCs to clear and reduce concurrent test traffic.

## CCH

## FIBER_CCH_001 : Gateway Unavailable
**Category:** CrossChainHub
**Severity:** Medium
**Description:** No CCH gateway peer is configured or reachable.
**Technical cause:** `send_btc` or `receive_btc` cannot create an order because no gateway is available.
**Common triggers:** Local DevKit network has no CCH operator; CCH RPC module enabled but gateway absent.
**Remediation:** Treat CCH as unavailable in local tests or configure a real CCH gateway.

## FIBER_CCH_002 : Unsupported CCH Asset
**Category:** CrossChainHub
**Severity:** Medium
**Description:** The requested CCH asset is outside the supported BTC/wrapped-BTC bridge.
**Technical cause:** CCH is an order-based BTC bridge, not a generic multi-asset router.
**Common triggers:** Requesting CKB, RUSD, or arbitrary UDT through CCH.
**Remediation:** Use native Fiber for non-BTC assets or request a BTC/wrapped-BTC flow only.

## FIBER_CCH_003 : Invoice Validation Failed
**Category:** CrossChainHub
**Severity:** Medium
**Description:** The supplied Bitcoin or Fiber invoice is malformed or amount mismatched.
**Technical cause:** The order remains invalid before it can advance from `Pending`.
**Common triggers:** Empty `btc_pay_req`; wrong network invoice; mismatched amount.
**Remediation:** Generate a fresh invoice on the correct network and retry.

## FIBER_CCH_004 : Incoming TLC Timeout
**Category:** CrossChainHub
**Severity:** High
**Description:** The incoming CCH leg timed out before the required TLCs arrived.
**Technical cause:** `Pending` order failed before incoming acceptance.
**Common triggers:** Missing incoming Fiber payment; expired invoice.
**Remediation:** Recreate the CCH order and complete the incoming leg before expiry.

## FIBER_CCH_005 : Outgoing Payment Failed
**Category:** CrossChainHub
**Severity:** High
**Description:** The outgoing CCH leg failed while trying to obtain or release the preimage.
**Technical cause:** The order reached `OutgoingInFlight` and then `Failed`.
**Common triggers:** Lightning payment failure; unreachable outgoing peer; insufficient bridge liquidity.
**Remediation:** Check gateway liquidity and retry with a new order.
