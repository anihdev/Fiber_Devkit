# Fiber DevKit Development Roadmap

This document describes planned post-hackathon development. Items are organised by
milestone. Each milestone represents a coherent slice of work that can be shipped
and tested independently before the next begins. Everything below is intentionally deferred,
not forgotten.

---

## Milestone 1: Hardening and Near-Term Improvements (~Month 1)

The first milestone addresses known limitations of the MVP that affect day-to-day
usability for developers actively building on Fiber.

### Random per-project node keys

Generated FNN node keys are currently deterministic for testnet repeatability but
are publicly predictable. A hardening pass will add random per-project key generation
at `fiber init` time and make `fiber reset` preserve keys by default unless the user
explicitly passes `--rotate-keys`. This makes DevKit safe to use across long-lived
testnet experiments without exposing predictable private key material.

### `fiber doctor <payment-hash>` lookup

`list_payments` is confirmed available in FNN v0.9.0-rc5 and returns payment status
and `failed_error` fields per payment. The implementation path is: query `list_payments`
on each configured node, match by `payment_hash`, extract `failed_error`, and feed it
into the existing `DiagnosticEngine`. The MVP `fiber doctor` intentionally accepts log
files, taxonomy codes, and raw error text only, payment-hash diagnosis was deferred
because it requires per-node query logic and ambiguity handling when multiple nodes
may or may not know the payment. `list_payments` makes this solvable without additional
persistence.

### `fiber inspect --peers`

`list_peers` is confirmed in FNN v0.9.0-rc5 and returns `Vec<PeerInfo>` with pubkey
and address per connected peer. Extending `fiber inspect` to show full peer pubkeys and
addresses when `--peers` is passed is one additional RPC call per node and requires no
new infrastructure. The MVP inspect already shows `peers_count` from `node_info`; full
peer detail is the natural next step.

### `fee-spike.yaml` scenario

The MVP cannot ship a reliable `fee-spike.yaml` because the current scenario action
set cannot deterministically force a fee-policy failure. Doing it properly requires at
least one of: fee-policy configuration in node templates, node-template fee overrides,
restart/reload handling after policy changes, or a preflight assertion confirming the
route is liquid and only fee policy can block the payment. `FIBER_FEE_001` is already
documented in `TAXONOMY.md` and the diagnostic engine can classify fee-related errors
from logs or raw error text. The scenario itself ships in Milestone 1 once deterministic
fee-policy control is confirmed.

### `send_payment` dry-run as an optional validation layer

`send_payment` with `dry_run: true` is confirmed in FNN v0.9.0-rc5. It checks whether
a route can be built and estimates the fee without executing the payment. In the current
MVP, `fiber predict` uses DevKit's own four-factor heuristic over `list_channels` as
its primary route confidence model and does not depend on dry-run. The dry-run field
is a future optional secondary validation layer, a way to confirm route buildability
against the live node's graph before returning the heuristic score. It should never
become the primary model because it says nothing about capacity direction or confidence
probability, only whether a path exists at all.

---

## Milestone 2: Observability and Monitoring (~Month 2)

This milestone adds the persistent monitoring and reporting layer that the MVP
deliberately deferred in favour of completeness across the core commands.

The read-only `fiber console` now ships as the Fiber DevKit Console: a local
visualization over existing DevKit JSON contracts. It is not persistent monitoring,
does not mutate node state, and does not replace `fiber watch`.

### Fiber DevKit Console evolution

The console should remain a thin browser shell over DevKit contracts, not a separate
dashboard product. Future work should deepen the read-only view before adding controls:

- topology graph over the same data used by `fiber inspect` and `fiber predict`
- scenario timeline and last-run report viewer over `report.md`, `logs.json`,
  `trace.json`, and `last-run.json`
- taxonomy explorer over the existing `DiagnosticEngine` entries
- route comparison panel for native Fiber, graph-topology fallback, and CCH readiness
- channel drilldowns with local/remote balances, state, enabled flag, and peer alias
- CCH readiness panel that explains whether a CCH actor, `[cch]` config section,
  Lightning/LND backend, Lightning liquidity, and wrapped BTC type script are present

The architectural boundary is that CLI and Rust modules remain the source of truth.
`fiber console` may display data from `visibility`, `RouteAnalyzer`,
`DiagnosticEngine`, reporter/tracer artifacts, and future `fiber watch` events, but it
should not invent a parallel backend, scheduler, persistence layer, or payment control
surface.

### `fiber watch`

A polling health monitor that checks node reachability, peer count changes, channel
state changes, weak route confidence, CCH unavailability, and watchtower health on a
configurable interval. Emits structured JSON events to stdout so they can be piped
directly into any observability stack.
The output format will be designed for integration with Prometheus push gateway,
Grafana Loki, and generic webhook forwarding, making DevKit the structured data layer
for any monitoring infrastructure a node operator already runs.

### `graph.svg` and `metrics.json` report artifacts

`fiber report` and `fiber run --report` currently write `report.md`, `logs.json`,
`trace.json`, and `last-run.json`. Two additional artifacts are roadmapped:

- `graph.svg` - a route visualisation showing the payment path as a directed graph
  with hop labels, channel capacities, and failure points highlighted. Suitable for
  embedding in GitHub issues and CI failure annotations.
- `metrics.json` - a structured summary of scenario run metrics: step durations,
  payment latencies, route scores, and assertion results. Intended for automated
  trend analysis across multiple runs.

Both were removed from the MVP to avoid SVG rendering scope during the two-week
build. The `report.md` and `trace.json` artifacts are sufficient for demonstration.

### Live OpenTelemetry OTLP export

`trace.json` is currently hand-written in OTel-compatible JSON span structure without
the `opentelemetry` SDK dependency. The forward-compatible structure means adding live
OTLP export is a future one-crate addition: add `opentelemetry-otlp` to `Cargo.toml`,
read `OTEL_EXPORTER_OTLP_ENDPOINT` from the environment, and emit spans to a live
collector. The SDK was deliberately excluded from the MVP to avoid compile-time
complexity during the hackathon window.

---

## Milestone 3: CCH Tier 2 and Cross-Chain Execution (~Month 3–4)

This milestone activates live CCH order execution once the external infrastructure
prerequisites can be reliably documented and tested.

### Live CCH order probing for `fiber predict --cross-chain`

The current `--cross-chain` output reports CCH availability and mechanism honestly
without populating `quotedOrder` because no live CCH actor is running in a standard
DevKit local network. When a developer configures a CCH-enabled node (see
`docs/cch-setup.md`), the implementation will probe `send_btc` with a minimal
well-formed request and, if an order is created, return real `fee_sats`, `status`,
and `expiry_delta_seconds` fields from the `CchOrderResponse`. It will never infer
CCH route probability or hop count, CCH is an order-based BTC/wrapped-BTC bridge,
not a route graph, and that distinction is structural rather than a limitation.

### CCH Tier 2 activation - `cch-routing.yaml`

Full activation requires: a `[cch]` config section in the FNN node config, a running
LND (or compatible) Lightning backend reachable from FNN, testnet Lightning liquidity
with a funded channel, and a wrapped BTC type script deployed on CKB testnet. The
complete configuration path is documented in `docs/cch-setup.md`. Once an environment
that satisfies these requirements is available, the implementation adds:

- A `cch-routing.yaml` scenario that exercises the full `send_btc` -> `get_cch_order`
  -> status polling lifecycle to `Success`
- A `[cch]` service section option in the generated node config template
- The `cch-routing.yaml` scenario listed in the reference library table in `README.md`
  with status `Active (requires CCH gateway)`

The taxonomy (`FIBER_CCH_001` through `FIBER_CCH_005`) and diagnostic engine CCH
handling are already shipped in the MVP and require no changes.

---

## Milestone 4: Ecosystem Expansion (~Month 4–6)

This milestone expands the scenario library, node template set, and diagnostic
taxonomy to cover the full range of Fiber network behaviour.

### Additional node templates

Current MVP ships hub and leaf only. Milestone 4 adds:

- `merchant` - high-inbound, auto-accept, low-fee policy; models a payment receiver
- `lsp-stub` - high-capacity hub with fee-rate configuration; models a Liquidity
  Service Provider
- `watchtower` - channel monitoring node with delegation configuration

### Additional scenarios

| Scenario | What it tests | Prerequisite |
|---|---|---|
| `fee-spike.yaml` | Fee policy failure | Milestone 1 fee-policy controls |
| `htlc-timeout.yaml` | HTLC expiry before preimage | Controllable HTLC timelock |
| `force-close.yaml` | Unilateral close and on-chain settlement | Chain observation layer |
| `watchtower-detection.yaml` | Watchtower detects and responds to stale state | Watchtower node template |
| `multi-asset-payment.yaml` | Payment using a non-CKB UDT asset type | Multi-asset channel setup |
| `rebalancing.yaml` | Circular rebalance restoring channel liquidity direction | Rebalance action type |

### Expanded taxonomy (25 -> 60+ codes)

The 25-code MVP taxonomy covers the most common failure classes. The structure is
intentionally expandable; adding a new code requires one entry in `taxonomy.rs` and
one entry in `TAXONOMY.md`. The 60+ code target adds:

- Multi-hop HTLC edge cases not covered by `FIBER_HTLC_001`
- Multi-asset routing failures (`FIBER_ASSET_002` through `FIBER_ASSET_005`)
- Watchtower interaction errors (`FIBER_CHAN_005` through `FIBER_CHAN_008`)
- Full CCH order lifecycle failure set beyond the five MVP codes
- Jamming mitigation detail codes under `FIBER_JAM_xxx`

### `fiber snapshot` and `fiber restore`

Network state persistence: save the current channel state, peer connections, and
balance distribution to a named snapshot; restore it to return to a known test
baseline without re-running `pnpm fund:nodes`. Valuable for stateful regression
testing where the same funded network topology needs to be tested repeatedly across
code changes.

---

## Milestone 5: Platform, Tooling, and Publishing (~Month 6+)

This milestone converts DevKit from a hackathon-origin tool into a maintained
ecosystem platform.

### Plugin API

A `trait ScenarioAction` interface so third parties can add custom scenario actions,
swap, rebalance, benchmark, custom RPC calls, chain observations; without modifying
DevKit's core. Each action implements a defined interface, reads from the scenario YAML,
and returns a typed `StepResult`. This converts DevKit from a fixed-function tool into
a platform the Fiber ecosystem can extend without forking. The scenario format is already
structured to support additional action types; the plugin API formalises the extension
boundary.

Reusable ecosystem primitives

Several DevKit artifacts are already specified as standalone documents rather than
implementation details: TAXONOMY.md and SCENARIO_FORMAT.md exist independently of
the code that currently implements them, and trace.json is written in an
OTel-compatible shape specifically so it isn't tied to DevKit's own tooling. This
milestone extends that intent into actual reusable interfaces:


Diagnostic taxonomy as a library — expose src/diagnostic as a crate/library
API so other Fiber tooling (node GUIs, operator dashboards, wallet debugging panels)
can translate raw FNN errors into the same structured explanations fiber doctor
produces, without depending on DevKit's CLI or network orchestration.
Scenario format as a shared test contract — SCENARIO_FORMAT.md is already a
versioned schema independent of ScenarioRunner's implementation. If other Fiber
tooling needs a way to express payment and routing test cases, DevKit's format and
runner can serve as a reference implementation rather than a closed internal format.


Neither of these depends on any other specific project adopting DevKit as a whole —
each is a independently useful surface a future contributor or another team
could depend on directly, whether or not they use any other part of DevKit.

### VSCode Extension

Run scenarios from the editor sidebar, see `fiber doctor --explain` diagnostic output
inline next to failing steps, and jump to the channel or hop that caused a failure in
a payment trace. The scenario YAML format and diagnostic JSON output are already the
right structure for this integration; the extension is a presentation layer, not a
protocol change.

### Browser scenario controls

`fiber console` already provides a read-only local browser view for node status,
prediction, taxonomy, and last-run artifacts. Future browser work should be scoped to
explicit scenario-triggering or replay workflows only after the CLI contracts are stable.
If controls are added, they should call the existing scenario runner and CLI contracts
with explicit local confirmation rather than issuing hidden raw RPC mutations from the
frontend. It must stay separate from `fiber watch`, which remains the real
monitoring/alerting engine.

### Binary rename consideration before crates.io publish

The hackathon binary is named `fiber`, and we will need to decide whether that name
could conflict with future official Fiber tooling or other published crates before we
publish to crates.io or encourage broader ecosystem adoption. The crate name
`fiber-devkit` is already the right one, and if a rename becomes necessary, the Cargo
`[[bin]]` name field is the only change required; the command examples and generated
CI files would be updated alongside it. We will make that decision in coordination
with the Fiber team before any public registry publish.

---

## Items That Will Not Be Built

The following items were evaluated and explicitly excluded:

- **`fiber simulate` as a live payment command**: DevKit is testing infrastructure,
  not a payment tool. `fiber simulate` remains dry-run only and delegates to the same
  route analysis logic as `fiber predict`. A live payment execution path would blur
  the infrastructure/product boundary the project is built around.
- **Alerting push integrations in MVP**: push alerting to PagerDuty, OpsGenie, or
  similar targets requires per-operator configuration that belongs in `fiber watch`
  (Milestone 2), not in the core CLI. `fiber predict` warning output and `fiber inspect`
  reachability status cover the structural detection layer in the MVP.
- **Mainnet support**: deterministic generated keys and testnet-only funding scripts
  make DevKit safe for testnet work only in its current form. Mainnet support requires
  random key generation (Milestone 1), explicit user acknowledgment of real-value risk,
  and auditing of the funding script logic. It is not a short addition.

Future ecosystem integration could expose DevKit's diagnostic taxonomy and scenario format as reusable libraries and specifications for other Fiber tooling, including node GUIs, operator dashboards, and future testing tools, without requiring adoption of DevKit as a whole.

---

*This roadmap reflects the project state at hackathon submission. Priorities may shift
based on Fiber Network protocol changes, community feedback, and follow-on grant scope.*
