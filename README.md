# Fiber DevKit

Fiber DevKit is a Rust CLI (`fiber`) for local Fiber Network development, scenario
testing, diagnostics, route prediction, report artifacts, and CI scaffolding.

It is not a wallet, merchant checkout, node management CLI, or replacement for `fnn-cli`. DevKit orchestrates multiple local FNN nodes so developers can reproduce
network behavior, run scenario tests, diagnose failures, and attach useful artifacts
to issues or CI runs.

## Hosted Demo

The GitHub Pages demo lives in [`docs/index.html`](docs/index.html). It embeds
self-hosted Asciinema cast files for the core CLI flow: network startup, funded
payment, low-liquidity diagnosis, route prediction, and taxonomy explanation.
The public [`docs/Devkit_screebshots/UI_actions/`](docs/Devkit_screebshots/UI_actions/) gallery collects full-resolution screenshots
of the UI and key actions: architecture, console, reports, CI, lifecycle, and hosted deployment.

Until the crate is published, `make setup` installs support-script dependencies and
installs the local `fiber` binary from this checkout.

## Setup

```bash
git clone https://github.com/anihdev/Fiber_Devkit.git
cd Fiber_Devkit
make setup
fiber --help
fiber --version
```

`make setup` runs `pnpm install` for the TypeScript/CKB support scripts and
`cargo install --path . --locked --force` for the Rust CLI. Cargo installs the
`fiber` binary into Cargo's binary directory. Run `make help` to list repository
setup and verification shortcuts.

## Quickstart: Unfunded Smoke Test

```bash
fiber init --nodes 3 --template hub-spoke
fiber up
fiber inspect
fiber validate --live
fiber run scenarios/network-smoke.yaml --report
fiber report --format md
fiber down
```

This path works on a fresh clone without testnet CKB. It validates Docker orchestration,
FNN RPC reachability, read-only network visibility, scenario execution, and report
artifact generation.

## Quickstart: Funded Payment Scenarios

Payment scenarios need testnet CKB on generated node keys:

```bash
fiber init --nodes 3 --template hub-spoke
cp .env.example .env
# Edit .env and set CKB_PRIVATE_KEY to a funded CKB testnet treasury key.
pnpm balances:nodes
pnpm fund:nodes
fiber up
fiber inspect
fiber run scenarios/basic-payment.yaml --report
fiber inspect node-1 --channels
fiber down
```

Generated deterministic node keys are testnet-only. Do not use them with mainnet or
long-lived funds. Run `pnpm balances:nodes` and `pnpm fund:nodes` after `fiber init` or
`fiber reset` and before `fiber up`; once FNN starts, it may rewrite the generated key
file into its own storage format.

## Commands

| Command | Purpose |
|---|---|
| `fiber --help` / `fiber <command> --help` / `fiber --version` | Discover CLI scope, command usage, and installed version |
| `fiber init --nodes 3 --template hub-spoke` | Generate `.fiber/config.toml`, node config, and Docker setup |
| `fiber validate [--live]` | Check Docker, config, ports, image cache, CKB RPC, and optional live node RPC |
| `fiber up` / `fiber down` / `fiber reset` | Start, stop, or rebuild the local Docker network |
| `fiber inspect [node-name] [--channels] [--json]` | Read-only node health, peer count, and channel-state view |
| `fiber console [--port 7717] [--open]` | Read-only local browser view over inspect, predict, taxonomy, and last-run data |
| `fiber run <scenario.yaml> [--report]` | Execute any scenario and persist `last-run.json`; `--report` writes the complete artifact set |
| `fiber doctor <log-file\|taxonomy-code\|raw-error> [--explain]` | Explain known Fiber failure classes |
| `fiber predict <from> <to> <amount> [--asset CKB] [--cross-chain]` | Score native Fiber route confidence without sending a payment |
| `fiber simulate <from> <to> <amount> --dry-run` | Compatibility dry-run path delegating to `predict` |
| `fiber report --format md\|json` | Regenerate all latest-run artifacts and print the selected Markdown or JSON path |
| `fiber ci init` | Scaffold `.github/workflows/fiber-ci.yml` |

## Development

Install or refresh the local CLI:

```bash
make help       # list repository setup and verification shortcuts
make setup      # fresh clone: pnpm support deps + local fiber CLI
make install    # refresh only the local fiber CLI after code changes
```

Run the local verification suite:

```bash
make check
```

Run an unfunded end-to-end smoke test:

```bash
make smoke
```

| Target | Purpose |
|---|---|
| `make help` | List available repository setup and verification shortcuts |
| `make setup` | Fresh-clone setup: install TypeScript support dependencies and local `fiber` CLI |
| `make install` | Install or refresh only the local `fiber` CLI |
| `make check` | CI-equivalent local verification: build, typecheck, format, test, clippy |
| `make smoke` | Unfunded local network smoke test with one startup retry and cleanup trap; writes raw JSONL to `/tmp/fiber-devkit-smoke.jsonl` |
| `make clean` | Remove Rust build artifacts with `cargo clean` |

Funded payment scenarios are not automated by Makefile because they require testnet CKB
and a local `.env` treasury key. See Quickstart: Funded Payment Scenarios and
`SCENARIO_FORMAT.md`.

## Reports

Every completed scenario run updates `.fiber/output/last-run.json`, including a plain run:

```bash
fiber run <scenario.yaml>
```

Add `--report` to any scenario to immediately generate the full report artifact set:

```bash
fiber run <scenario.yaml> --report

# Examples
fiber run scenarios/network-smoke.yaml --report
fiber run scenarios/basic-payment.yaml --report
```

This writes:

- `.fiber/output/report.md`: readable scenario outcome with grouped failure causes, likely
  triggers, and complete remediation steps. Expected failures are explained separately.
- `.fiber/output/logs.json`: full structured run result, including embedded diagnosis
  metadata for observed RPC failures.
- `.fiber/output/trace.json`: OTel-compatible span JSON.
- `.fiber/output/last-run.json`: persisted source for `fiber report`.

After any completed run, `fiber report` regenerates **all four artifacts** from
`last-run.json`. The format flag only selects which artifact path is printed:

```bash
fiber report --format md    # regenerate everything; print the report.md path
fiber report --format json  # regenerate everything; print the logs.json path
```

`--format md` selects the human-readable Markdown analysis. `--format json` selects the
full structured `RunResult`; it does not convert the Markdown report into JSON or limit
generation to one file.

Every unexpected failed step is included in **Failure Analysis** with what happened, why it
failed, and what to do next. Repeated taxonomy codes are grouped to keep reports readable.
Failures that were declared with `expect: failure` appear under **Expected Failure Analysis**
so successful negative tests still preserve their cause and remediation guidance.

## Inspect Network State

```bash
fiber up
fiber inspect
fiber inspect node-1 --channels
fiber inspect --json
fiber down
```

`inspect` is read-only. It uses configured nodes from `.fiber/config.toml`, calls
`node_info` and `list_channels`, and continues across unreachable nodes so partial
network state is still visible.

## Local Console

```bash
fiber up
fiber console --port 7717
# open http://127.0.0.1:7717/
fiber down
```

`fiber console` opens the Fiber DevKit Console: a read-only local browser view over
existing DevKit data. It shows node reachability, channel state, route prediction JSON,
taxonomy hints, and the latest scenario artifact when present. It does not start nodes,
run scenarios, fund keys, open channels, or send payments. The Fiber DevKit Console is
responsive, so the same local read-only console can be viewed comfortably on desktop or
mobile-sized screens during demos/debugging.

## Diagnostics

`fiber doctor` intentionally accepts log files, taxonomy codes, and raw error text:

```bash
fiber run scenarios/low-liquidity.yaml 2>&1 | tee failure.log
fiber doctor failure.log --explain
fiber doctor FIBER_LIQ_001 --explain
```

Payment-hash lookup is roadmap work because a payment hash alone does not identify which
local node/RPC endpoint owns the historical payment state.

## Reference Scenarios

| Scenario | Requires funded keys | Purpose |
|---|---:|---|
| `network-smoke.yaml` | No | Clean unfunded RPC and graph smoke test |
| `basic-payment.yaml` | Yes | Funded single-hop success |
| `multi-hop-routing.yaml` | Yes | Hub-spoke multi-hop payment |
| `low-liquidity.yaml` | Yes | Structured insufficient-liquidity failure |
| `peer-offline.yaml` | No | Step-level unreachable endpoint reporting |
| `channel-exhaustion.yaml` | Yes | Low-confidence prediction and over-capacity failure |

`fee-spike.yaml`, watchtower scenarios, live CCH scenarios, graph SVG output, random
key mode, and payment-hash doctor lookup are roadmap items. Fee-spike simulation is deferred
because the current scenario action set cannot deterministically manipulate fee policy;
`FIBER_FEE_001` is documented and the diagnostic engine can still classify fee-related
errors from logs or raw error text.

## CI Integration

```bash
fiber ci init
```

This creates `.github/workflows/fiber-ci.yml` with Rust formatting, tests, clippy,
TypeScript support-script typechecking, DevKit initialization, validation, and the
unfunded `network-smoke.yaml --report` workflow.

## Architecture

- `src/network`: Docker/bollard orchestration and generated FNN config.
- `src/rpc`: thin JSON-RPC 2.0 client for FNN.
- `src/scenario`: YAML parser and step runner.
- `src/visibility`: shared read-only node/channel inspection data.
- `src/diagnostic`: taxonomy-backed failure diagnosis.
- `src/route`: native Fiber route prediction plus honest CCH availability output.
- `src/console`: embedded read-only local browser console.
- `src/reporter` and `src/tracer`: Demo 5 report artifacts.

## Specs

- `SCENARIO_FORMAT.md`: implemented YAML scenario schema.
- `TAXONOMY.md`: MVP diagnostic taxonomy.
- `docs/cch-setup.md`: CCH activation notes and source references.
- `ROADMAP.md`: post-hackathon hardening and intentionally deferred work.

## License

MIT. See [LICENSE](LICENSE).

## Important Public References

- Fiber docs: https://www.fiber.world/docs
- Fiber API reference: https://www.fiber.world/docs/api-reference
- Fiber config reference: https://www.fiber.world/docs/operate/config-reference
- FNN source: https://github.com/nervosnetwork/fiber
- Fiber scripts: https://github.com/nervosnetwork/fiber-scripts
- CCH setup notes: `docs/cch-setup.md`
- Hackathon announcement: https://talk.nervos.org/t/gone-in-60ms-fiber-network-infrastructure-hackathon-announcement/10418
- Fiber AMA recap: https://talk.nervos.org/t/the-fiber-network-ama-recap/10294
