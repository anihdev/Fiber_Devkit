# Fiber DevKit Agent Guide

This file is the public contributor and AI-agent guide for Fiber DevKit. It should stay
accurate to the repository as it exists now, not to private planning notes or hackathon documents.

## WHAT WE ARE BUILDING

Fiber DevKit is the development, testing, diagnostics, and operational intelligence toolkit
for Fiber: the infrastructure a Fiber developer can install first to create a local network,
run reproducible scenarios, inspect state, diagnose failures, predict routes, and generate
CI-friendly reports.

## Working Rule

Before adding or expanding a feature, apply this filter:

> Could the Fiber team realistically recommend or merge this six months from now?

If the answer is yes, keep it small and aligned with the current architecture. If the answer
is no, defer it.

### The workflow it answers end-to-end

```
Create Network -> Run Scenario -> Observe Payment -> Diagnose Failure -> Recommend Fix -> Generate Report
```

## Public Docs

The public project docs are:

- `README.md`
- `AGENTS.md`
- `SCENARIO_FORMAT.md`
- `TAXONOMY.md`
- `ROADMAP.md`
- `docs/`

Private planning, implementation notes, submission drafts, and presentation scripts are not
public source material and should not be referenced from product docs or code:

- `task.md`
- `PERSONAL_NOTES.md`
- `IMPLEMENTATION_NOTES.md`
- `submission/`

Those paths are intentionally ignored in `.gitignore`.

## What This Project Is

Fiber DevKit is a single Rust CLI package:

- Cargo package: `fiber-devkit`
- Binary: `fiber`
- Main workflow: create network -> run scenario -> inspect state -> diagnose failure -> predict route -> generate report

It is not:

- a wallet or wallet SDK
- a merchant checkout product
- a replacement for `fnn-cli`
- a general-purpose live operations CLI
- a standalone public network explorer
- a replacement for official Fiber SDKs

`fiber console` is intentionally a read-only local visualization over existing DevKit data.
It must not grow into a separate product surface without a deliberate design pass.

## Setup

Fresh clone:

```bash
make setup
fiber --help
```

`make setup` installs TypeScript support-script dependencies with `pnpm install` and installs
the local Rust CLI with `cargo install --path . --locked --force`.

Useful local commands:

```bash
make help
make check
make smoke
make install
```

`make smoke` is unfunded by design. Funded scenarios require a local `.env` with a funded
testnet `CKB_PRIVATE_KEY`.

## Verification

For code changes, run:

```bash
cargo fmt --check
cargo test --locked
cargo clippy --locked -- -D warnings
pnpm typecheck
git diff --check
```

For end-to-end local smoke:

```bash
make smoke
```

For manual live checks:

```bash
fiber init --nodes 3 --template hub-spoke
fiber up
fiber inspect
fiber run scenarios/network-smoke.yaml --report
fiber report --format md
fiber down
```

Always stop containers after live tests with `fiber down`.

## REPOSITORY STRUCTURE

```
fiber-devkit/
├── src/
│   ├── cli/                   # clap subcommand definitions
│   │   ├── init.rs
│   │   ├── validate.rs        # pre-flight environment checks
│   │   ├── up.rs
│   │   ├── down.rs
│   │   ├── reset.rs
│   │   ├── inspect.rs
│   │   ├── console.rs
│   │   ├── run.rs
│   │   ├── simulate.rs
│   │   ├── predict.rs         # includes --cross-chain flag
│   │   ├── doctor.rs
│   │   ├── report.rs
│   │   └── ci.rs
│   ├── network/               # Docker/bollard orchestration
│   │   ├── mod.rs
│   │   ├── manager.rs         # NetworkManager struct
│   │   ├── compose.rs         # FNN config + Docker setup generator
│   │   └── templates.rs       # hub and leaf node template definitions (MVP)
│   ├── scenario/              # YAML scenario parser + executor
│   │   ├── mod.rs
│   │   ├── parser.rs
│   │   ├── runner.rs
│   │   └── types.rs           # Scenario, Step, Assertion, RunResult types
│   ├── rpc/                   # Fiber node RPC client
│   │   ├── mod.rs
│   │   ├── client.rs          # JSON-RPC 2.0 HTTP client
│   │   └── types.rs           # FNN response type structs (incl. CCH RPC types)
│   ├── tracer/                # Payment event collection
│   │   ├── mod.rs
│   │   └── events.rs          # typed event definitions
│   ├── diagnostic/            # Error taxonomy engine
│   │   ├── mod.rs
│   │   ├── engine.rs          # parse, humanize, diagnose, explain
│   │   └── taxonomy.rs        # all error code constants + metadata + explainTemplate
│   ├── route/                 # Route intelligence
│   │   ├── mod.rs
│   │   ├── analyzer.rs        # can_pay, find_paths, score_route, suggest_alternative
│   │   └── cch.rs             # compare_routes, CchPathResult, RouteComparison
│   ├── reporter/              # Output artifact generation
│   │   ├── mod.rs
│   │   └── formats.rs         # report.md, logs.json, trace.json
│   ├── console/               # embedded read-only local browser console
│   │   ├── mod.rs
│   │   ├── server.rs          # tokio TcpListener HTTP/1.1 server, GET-only
│   │   └── assets.rs          # include_str! HTML/CSS/JS assets
│   ├── config.rs              # .fiber/config.toml read/write
│   ├── visibility.rs          # shared inspect/console node visibility data
│   └── main.rs                # CLI entry point
├── console/                   # embedded frontend source assets
│   ├── index.html
│   ├── app.js
│   └── style.css
├── scenarios/                 # MVP scenarios — created during Demos 2–4, see Section 10
│   ├── network-smoke.yaml
│   ├── basic-payment.yaml
│   ├── multi-hop-routing.yaml
│   ├── low-liquidity.yaml
│   ├── channel-exhaustion.yaml
│   └── peer-offline.yaml
│   # fee-spike.yaml is roadmap until fee-policy controls are deterministic with
│   # the existing scenario action set.
│   # cch-routing.yaml is NOT scaffolded here. Tier 2 live CCH is roadmap until
│   # CCH actor/service and Lightning/LND backend activation are configured.
├── templates/                 # FNN Docker node config templates (MVP: hub + leaf only)
│   ├── hub.toml
│   └── leaf.toml
├── .github/
│   └── workflows/
│       └── fiber-ci-example.yml  # Scaffolded by `fiber ci init`
├── TAXONOMY.md                # Fiber Payment Failure Taxonomy spec (first-class artifact)
├── SCENARIO_FORMAT.md         # YAML scenario format specification (first-class artifact)
├── README.md
├── Makefile                   # contributor shortcuts: make check, make smoke, make clean
└── Cargo.toml
```

`Makefile` is polish only. It wraps local verification and unfunded smoke testing for convenience; canonical workflows remain the README, generated CI, and the `fiber` CLI. 

Generated local state lives under `.fiber/` and must not be committed.

## CLI Surface

Current shipped commands:

```text
fiber init
fiber validate
fiber up
fiber down
fiber reset
fiber inspect
fiber console
fiber run
fiber predict
fiber simulate
fiber doctor
fiber report
fiber ci init
```

Keep command help concrete. `fiber --help` should show the high-level command list, while
`fiber <command> --help` should explain command-specific flags and examples.

Reporting help must distinguish persistence from rendering: every completed `fiber run`
updates `last-run.json`; `fiber run --report` writes the complete artifact set; and
`fiber report --format md|json` always regenerates all artifacts while selecting only the
Markdown or structured-JSON path printed to stdout.

## Runtime RPC Surface

The implemented Rust RPC client currently exposes:

- `node_info`
- `list_channels`
- `list_channels` with options, wrapped as `list_pending_channels` and `list_all_channels`
- `open_channel`
- `send_payment`
- `get_payment`
- `graph_nodes`
- `graph_channels`

Do not claim a method is implemented just because it exists upstream. Confirm it is exposed
in `src/rpc/client.rs` before wiring a feature to it.

Confirmed upstream RPCs that are roadmap enablers, not current DevKit runtime calls:

- `list_peers`: future `fiber inspect --peers`
- `list_payments`: future `fiber doctor <payment-hash>`
- `send_payment` with `dry_run: true`: future validation layer for `fiber predict`
- `send_btc`, `receive_btc`, `get_cch_order`: future live CCH Tier 2 work
- `subscribe_store_changes`: future `fiber watch`

## Scenario Rules

`fiber run` executes YAML scenarios against the local network. Keep scenarios deterministic:

- Unfunded scenarios must run on a fresh clone.
- Funded scenarios must clearly require testnet CKB and `.env`.
- Expected failures should be explicit and should feed diagnostic output when possible.
- Do not add scenarios that require uncontrolled timing, fee policy mutation, or external
  infrastructure unless the precondition is documented.

Current public scenario docs live in `SCENARIO_FORMAT.md`.

## Diagnostics

`fiber doctor` accepts:

- scenario JSONL log files
- taxonomy codes, for example `FIBER_LIQ_001`
- raw error text

It does not currently accept a payment hash. Payment-hash lookup is roadmap work because the
tool must query configured nodes with `list_payments`, find the matching `payment_hash`, and
feed `failed_error` into the diagnostic engine.

Current taxonomy docs live in `TAXONOMY.md`.

Markdown reports must preserve the diagnostic contract for every failed outcome: what
happened, why it failed, and what to do next. Group repeated taxonomy codes, include all
remediation steps, explain expected failures separately, and provide useful fallback
guidance when no taxonomy diagnosis is attached. JSON artifacts should retain the complete
structured diagnosis rather than replacing it with presentation-only text.

## Route Intelligence

`fiber predict` is a pre-payment confidence tool. It does not send payments. The route score
is a transparent heuristic based on local channel data or topology fallback, not a statistical
model.

When route data comes only from graph topology, confidence is capped and warnings should make
clear that graph capacity is not spendable directional liquidity.

`fiber simulate --dry-run` delegates to prediction. It is not a live payment command.

## CCH Handling

Fiber DevKit ships CCH Tier 1 support:

- `fiber predict --cross-chain` returns native Fiber analysis plus an honest CCH availability
  block.
- CCH diagnostic taxonomy entries classify known CCH order failure modes.
- `docs/cch-setup.md` documents live CCH activation requirements and references upstream
  source.

Tier 2 live order execution is roadmap work. It requires a real CCH actor, `[cch]` config,
Lightning/LND credentials, Lightning liquidity, and a wrapped BTC type script. A local DevKit
network without that backend should report CCH order methods as unavailable rather than
pretending to bridge.

## Funding Model

Generated FNN node keys live under `.fiber/nodes/<node>/ckb/key`. Payment/channel scenarios
use real testnet CKB funding on those keys.

Support scripts:

- `pnpm balances:nodes`
- `pnpm fund:nodes`

Never print private keys. Never encourage mainnet use of generated deterministic keys.

## Console Rules

`fiber console` is local, read-only, and GET-only. It may display:

- node reachability
- peer counts
- channel state
- route prediction output
- taxonomy entries
- latest report artifact data

It must not:

- start or stop nodes
- fund keys
- open or close channels
- send payments
- mutate scenario state
- expose secrets

The console frontend should stay responsive for desktop and mobile-sized screens, but it is a
local developer console, not a mobile product.

## 4. RPC INTERFACE — COMPLETE REFERENCE

This is the primary integration surface for DevKit. All node interaction goes through here.

### Transport
```
POST http://<node_address>:<port>/
Content-Type: application/json
```
Default local address: `http://127.0.0.1:8227`

All modules share the same endpoint. There is no per-module URL path.

### Request format (JSON-RPC 2.0)
```json
{
  "jsonrpc": "2.0",
  "method": "node_info",
  "params": [],
  "id": 1
}
```
`params` is always an array. Even methods with no arguments send `[]`.
Methods with arguments send a single-element array containing the params object.

### Response format
```json
{ "jsonrpc": "2.0", "result": { ... }, "id": 1 }
{ "jsonrpc": "2.0", "error": { "code": -32600, "message": "..." }, "id": 1 }
```

### Authentication (Biscuit tokens)
- **Localhost / LAN (127.0.0.1, 192.168.x.x):** Auth is OPTIONAL. If no `biscuit_public_key`
  is configured, all local requests are accepted. **DevKit local dev environments need no auth.**
- **Public addresses (0.0.0.0):** Auth is MANDATORY. The node will refuse to start without
  `rpc.biscuit_public_key` set.
- When required, send: `Authorization: Bearer <base64_biscuit_token>`
- Permission model: `read("<resource>")` and `write("<resource>")` Datalog facts.
  Example: `send_payment` requires `write("payments")`.

### Concrete RPC example
```bash
curl -X POST http://127.0.0.1:8227 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "node_info",
    "params": [],
    "id": 1
  }'
```

### Config reference
```yaml
rpc:
  listening_addr: "127.0.0.1:8227"
  biscuit_public_key: "ed25519/..."   # omit for localhost-only nodes
  enabled_modules:
    - cch
    - channel
    - graph
    - info
    - invoice
    - payment
    - peer
    - watchtower
  cors_enabled: true
```

## Code Style

- Prefer existing repo patterns over new abstractions.
- Add file-level comments for new Rust source files explaining ownership and boundaries.
- Use doc comments for public structs, enums, and methods when contracts are not obvious.
- Keep comments focused on non-obvious behavior and tradeoffs.
- Avoid referencing private notes from public code or docs.
- Keep public docs conservative: describe shipped behavior first, roadmap second.

## Safe Editing

- Do not delete or rewrite generated state with raw shell commands when `fiber reset` or
  `fiber down` owns that lifecycle.
- Do not commit `.env`, `.fiber/`, private notes, or submission drafts.
- Do not add a new external service dependency without documenting why it is needed.
- Do not make funded workflows run implicitly from Makefile or CI.

## REFERENCE LINKS (read before writing any code)

### Official Fiber documentation
- What is Fiber: https://www.fiber.world/docs
- How Fiber Works: https://www.fiber.world/docs/how-it-works
- RPC Overview: https://www.fiber.world/docs/api-reference
- SDK Docs: https://www.fiber.world/docs/build/sdk
- Toolchain Overview: https://www.fiber.world/docs/build/toolchain
- Config Reference: https://www.fiber.world/docs/operate/config-reference
- Node Backup: https://www.fiber.world/docs/operate/backup
- Public Nodes: https://www.fiber.world/docs/operate/connect-nodes
- Biscuit Auth: https://www.fiber.world/docs/concept/security/biscuit-auth
- Watchtower: https://www.fiber.world/docs/concept/security/watchtower
- Cross-Chain HTLC: https://www.fiber.world/docs/res/cross-chain-htlc
- Gossip Protocol: https://www.fiber.world/docs/res/gossip-protocol
- Fiber Glossary: https://www.fiber.world/docs/res/glossary
- Fiber Cheat Code: https://www.fiber.world/docs/res/cheat-code

### Protocol and tooling source
- FNN (Fiber Network Node — Rust): https://github.com/nervosnetwork/fiber
- Fiber Scripts (on-chain): https://github.com/nervosnetwork/fiber-scripts
- fiber-ffi (Rust FFI bindings): https://github.com/joii2020/fiber-ffi
- fiber-pay SDK + CLI (community TS): https://github.com/RetricSu/fiber-pay
- FNN repo AGENTS.md (build/contribution conventions for working in the FNN codebase
  itself — read this if Day 0 investigation requires building or running FNN from source):
  https://github.com/nervosnetwork/fiber/blob/v0.9.0-rc5/AGENTS.md

### CCH-specific source (read these before writing any CCH-related code — Demo 3/4/Tier 2)
- CCH RPC trait + handler implementation: confirms `send_btc`, `receive_btc`,
  `get_cch_order` as the complete client-facing method set, plain JSON-RPC:
  https://github.com/nervosnetwork/fiber/blob/v0.9.0-rc5/crates/fiber-lib/src/rpc/cch.rs
- CCH JSON types: `CchOrderResponse`, `CchOrderStatus`, `CchInvoice`, and all three
  params structs — the authoritative field list, more current than this document:
  https://github.com/nervosnetwork/fiber/blob/v0.9.0-rc5/crates/fiber-json-types/src/cch.rs
- CCH actor/order lifecycle implementation (read once you need to understand *why*
  an order is stuck in a given status, not just what the status enum says):
  https://github.com/nervosnetwork/fiber/tree/v0.9.0-rc5/crates/fiber-lib/src/cch
- Bruno e2e test collections — real, runnable request/response examples, faster than
  building payloads from scratch by reading struct definitions alone:
  https://github.com/nervosnetwork/fiber/tree/v0.9.0-rc5/tests/bruno/e2e/cross-chain-hub
  https://github.com/nervosnetwork/fiber/tree/v0.9.0-rc5/tests/bruno/e2e/cross-chain-hub-separate
- v0.9.0-rc5 release notes (CCH order status tracking + pending-expiry reconciliation
  fixes — read this to know what changed right before the hackathon started):
  https://github.com/nervosnetwork/fiber/releases/tag/v0.9.0-rc5

### Hackathon resources
- Hackathon docs repo: https://github.com/RetricSu/fiber-hackathon-docs
- Hackathon resources.md: https://github.com/RetricSu/fiber-hackathon-docs/blob/master/resources.md
- Hackathon onboarding.md: https://github.com/RetricSu/fiber-hackathon-docs/blob/master/onboarding.md
- Fiber game tutorial: https://github.com/RetricSu/fiber-hackathon-docs/blob/master/tutorials/fiber-game.md
- Hackathon announcement: https://talk.nervos.org/t/gone-in-60ms-fiber-network-infrastructure-hackathon-announcement/10418
- CKBoost registration: https://ckboost.netlify.app/campaign/0x3ba194e011b30817dfaf38bab946f55357a8bb32ef7dd1bbcf0608a59a132e6d

### Ecosystem context
- Fiber AMA recap (May 2026): https://talk.nervos.org/t/the-fiber-network-ama-recap/10294
- x402 integration PR: https://github.com/nervosnetwork/fiber/pull/1301
- Fiber Dashboard: https://dashboard.fiber.channel/
- CKB testnet faucet: https://faucet.nervos.org/
- Nervos docs: https://docs.nervos.org
- Lightning Builders Guide (for inspiration): https://docs.lightning.engineering/
