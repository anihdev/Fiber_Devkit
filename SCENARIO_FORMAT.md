# Fiber DevKit Scenario Format

This document describes the MVP YAML schema accepted by `fiber run`.
It only documents implemented MVP fields and actions. 

This format is maintained as a standalone specification, independent of DevKit's own scenario runner implementation. Other Fiber tooling authoring payment or routing test cases can adopt this schema directly.

## Top-Level Fields

```yaml
name: basic-payment
description: Optional human-readable summary.
nodes:
  alice:
    node: node-1
    template: hub
  bob:
    node: node-2
    template: leaf
channels:
  - from: alice
    to: bob
    capacity: "199 CKB"
    public: false
    one_way: true
steps:
  - action: list_channels
    node: alice
  - action: pay
    from: alice
    to: bob
    amount: "1 CKB"
    expect: success
assertions:
  - all_steps_passed
```

- `name` is required.
- `description` is optional.
- `nodes` is required and maps scenario aliases to live network nodes.
- `channels` is optional. Each channel is opened before `steps` run and is reported as a setup step.
- `steps` is required and runs in order.
- `assertions` is optional. If omitted, `all_steps_passed` is assumed.

Unknown fields are rejected.

## Nodes

Each node alias supports:

- `node`: name from `.fiber/config.toml`, such as `node-1`.
- `endpoint`: explicit JSON-RPC endpoint, used for negative connectivity scenarios.
- `template`: optional expected template, `hub` or `leaf`.

Exactly one of `node` or `endpoint` should be provided. If `template` is present with
`node`, the runner verifies it against `.fiber/config.toml`.

## Amounts

Amounts are strings in CKB, for example:

- `"1 CKB"`
- `"0.5 CKB"`
- `"199 CKB"`

The parser converts these to shannons using `1 CKB = 100000000 shannons`.

## Channel Setup

```yaml
channels:
  - from: alice
    to: bob
    capacity: "199 CKB"
    public: false
    one_way: true
```

Channel setup calls `open_channel` from `from` to `to`, using the target node's
`node_info.pubkey`. `public` defaults to `true`; `one_way` defaults to `false`.
When `one_way: true`, FNN requires `public: false`. In that mode, the channel can
only send payment from the opener toward the acceptor, but the acceptor still reserves
CKB during auto-accept. Generated Demo 1 templates auto-accept channels of at least
1 CKB and contribute the required CKB reserve from the accepting node's deterministic
funding key. Each participating generated node key must have enough testnet CKB for
its requested capacity, reserve, and on-chain fees. The runner waits until the channel
appears ready in `list_channels` before continuing.

For CKB channels, FNN reserves part of `funding_amount` for shutdown capacity and
fees before exposing liquid channel balance. With the pinned v0.9.0-rc5 config, a
`199 CKB` one-way channel provides about `100 CKB` outbound liquidity.

Fund generated node keys before running channel/payment scenarios:

```bash
fiber init --nodes 3 --template hub-spoke
pnpm balances:nodes
pnpm fund:nodes
fiber up
fiber run scenarios/basic-payment.yaml
```

`pnpm fund:nodes` reads `CKB_PRIVATE_KEY` from repo-root `.env`, uses `CKB_RPC_URL`
when set or `https://testnet.ckb.dev/rpc` by default, and tops generated node keys up
to `DEVKIT_NODE_FUND_CKB`, default `500` CKB. It waits for the funding transaction to
commit before returning. Generated FNN configs keep their own CKB RPC default in
`src/config.rs`. Generated node keys are deterministic and publicly predictable, so
this workflow is testnet-only. Fund after `fiber init` or `fiber reset` and before
`fiber up`; once FNN starts it may rewrite its key path into an internal binary format.

## Step Actions

### `node_info`

```yaml
- action: node_info
  node: alice
  expect: success
```

Calls `node_info` on the node.

### `list_channels`

```yaml
- action: list_channels
  node: alice
  expect: success
```

Calls `list_channels` on the node.

### `open_channel`

```yaml
- action: open_channel
  from: alice
  to: bob
  capacity: "199 CKB"
  public: false
  one_way: true
  expect: success
```

Opens a channel during the step phase. This has the same semantics as top-level
`channels`.

### `pay`

```yaml
- action: pay
  from: alice
  to: bob
  amount: "1 CKB"
  expect: success
```

Calls `send_payment` with `target_pubkey`, `amount`, and `keysend: true`. If the
initial response is not final, the runner polls `get_payment` by `payment_hash`.

### `predict`

```yaml
- action: predict
  from: alice
  to: bob
  amount: "1 CKB"
  asset: CKB
  cross_chain: false
  expect_probability_above: 0.85
```

Runs the same route analyzer used by `fiber predict` without sending a payment.
`asset` defaults to `CKB`; `cross_chain` defaults to `false`. When `cross_chain: true`,
the step output includes `nativeFiber` plus `cchBridged`. `cchBridged` is an honest
CCH bridge availability/mechanism statement only; it does not contain route paths,
hop counts, or probability fields.

If prediction must fall back from `list_channels` local balances to `graph_channels`,
the result is topology-only. Graph channel capacity is not treated as proven
directional liquidity, so the analyzer labels the route `graph_topology`, caps
confidence, and emits a warning.

Optional prediction assertions:

- `expect_probability_above`: native probability must be greater than this value.
- `expect_probability_below`: native probability must be less than this value.

### `graph_nodes`

```yaml
- action: graph_nodes
  node: alice
  expect: success
```

Calls `graph_nodes` on the node.

### `graph_channels`

```yaml
- action: graph_channels
  node: alice
  expect: success
```

Calls `graph_channels` on the node.

## Expectations

Each step supports:

- `expect: success` (default)
- `expect: failure`

A step passes when the observed outcome matches `expect`. Expected failures are useful
for connectivity and route-confidence checks; unexpected failures make the scenario
exit non-zero.

## Assertions

The MVP supports one assertion:

- `all_steps_passed`

It passes when every setup step and scenario step matched its expected outcome.

## Output

`fiber run` prints one JSON object per step to stdout, followed by a final summary
JSON object. Exit code `0` means all assertions passed. A non-zero exit means at
least one step or assertion failed.

`fiber run <scenario.yaml> --report` also writes:

- `.fiber/output/report.md`: human-readable scenario summary.
- `.fiber/output/logs.json`: full structured `RunResult`.
- `.fiber/output/trace.json`: OTel-compatible span document for scenario steps.
- `.fiber/output/last-run.json`: persisted run result used by `fiber report`.

`fiber report --format md` or `fiber report --format json` regenerates artifacts from
the most recent persisted run and prints the requested artifact path.

## Reference Scenarios

- `scenarios/network-smoke.yaml` runs on a clean generated network and does not require funded channels.
- `scenarios/basic-payment.yaml` and `scenarios/low-liquidity.yaml` create real channels, so the participating deterministic CKB keys must be funded on testnet.
- `scenarios/multi-hop-routing.yaml` and `scenarios/peer-offline.yaml` exercise the same MVP action set against the generated hub-spoke topology.
- `scenarios/channel-exhaustion.yaml` demonstrates a low route-confidence prediction before attempting an over-capacity payment.
- `scenarios/fee-spike.yaml` is roadmap until fee-policy controls can be made deterministic without expanding the MVP scenario action set.
