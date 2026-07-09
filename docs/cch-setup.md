# CCH Setup and Activation Notes

This document records the confirmed Cross-Chain Hub facts for Fiber DevKit against FNN
`v0.9.0-rc5`. It is an activation guide for future Tier 2 live CCH work.

## What DevKit Ships

DevKit ships Tier 1 CCH coverage without requiring a live CCH gateway:

- `fiber predict --cross-chain`
- CCH taxonomy codes in `TAXONOMY.md`
- `fiber doctor` diagnosis for CCH failure text
- Documentation for the CCH RPC surface and activation path

DevKit does not ship live CCH order execution, `cch-routing.yaml`, or CCH gateway management
in the MVP.

## Current Framing

Fiber DevKit covers CCH at two levels.

At the infrastructure level (Tier 1, shipped), `fiber predict --cross-chain` compares native
Fiber route analysis against CCH availability and bridge mechanism; `fiber doctor` classifies
CCH failure modes using taxonomy codes derived from the real `CchOrderStatus` state machine;
and `fiber inspect` confirms local FNN node/RPC health so CCH probe results can be interpreted
correctly. The MVP `inspect` command does not probe CCH gateway status directly.

At the execution level (Tier 2, documented), CCH requires a running CCH actor/service connected
to Lightning infrastructure: CCH config/service activation, LND credentials, a funded testnet
Lightning channel, and a wrapped BTC type script. The activation path is documented below.

DevKit's Tier 1 CCH coverage works in any local environment. Tier 2 live order execution
requires external Lightning infrastructure that is outside the scope of a default local
developer environment.

## Why Local DevKit Returns Method Not Found

Generated DevKit nodes run FNN and expose ordinary JSON-RPC methods such as `node_info`, but
CCH order methods require a running CCH actor.

`rpc.enabled_modules: cch` only allows the RPC module. It does not start the actor by itself.
The generated DevKit nodes do not include CCH service/config or an LND backend, so the CCH
actor is absent and the CCH methods are not registered.

## Verified Local Probe

On a local DevKit network using FNN `v0.9.0-rc5`:

| Probe | Observed result | Interpretation |
|---|---|---|
| `node_info` | Worked | FNN node, Docker port mapping, and JSON-RPC are healthy. |
| `send_btc` | `Method not found` | CCH client RPC method is not registered because no CCH actor/service is active. |
| `receive_btc` | `Method not found` | CCH client RPC method is not registered because no CCH actor/service is active. |
| `get_cch_order` | `Method not found` | CCH client RPC method is not registered because no CCH actor/service is active. |

That combination confirms the nodes were reachable and the blocker was CCH actor activation,
not general FNN or Docker availability.

## Confirmed CCH RPC Methods

The client-facing CCH methods confirmed from the `v0.9.0-rc5` source are:

- `send_btc`: `btc_pay_req`, `currency`
- `receive_btc`: `fiber_pay_req`
- `get_cch_order`: `payment_hash`

## Confirmed Currency Enum

- `Fibb`: Fiber mainnet
- `Fibt`: Fiber testnet
- `Fibd`: Fiber devnet

## Confirmed CchOrderResponse Fields

- `timestamp`
- `expiry_delta_seconds`
- `wrapped_btc_type_script`
- `incoming_invoice`
- `outgoing_pay_req`
- `payment_hash`
- `amount_sats`
- `fee_sats`
- `status`

## Confirmed CchOrderStatus State Machine

The normal success path is:

```text
Pending -> IncomingAccepted -> OutgoingInFlight -> OutgoingSuccess -> Success
```

`Failed` can occur as a terminal failure state.

## Requirements for Tier 2 Activation

Tier 2 live CCH testing requires:

- CCH service/config section
- LND or compatible Lightning backend
- LND endpoint reachable from FNN
- TLS cert and macaroon credentials
- Testnet Lightning liquidity/channel
- Wrapped BTC type script on CKB
- Valid BTC Lightning invoice or valid Fiber invoice



## Probe Sequence Once Configured

Level A checks that the methods are registered:

```bash
curl -X POST http://127.0.0.1:8227 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"send_btc","params":[{"btc_pay_req":"","currency":"Fibt"}],"id":1}'
```

Expected result after CCH activation: a validation error, not `Method not found`.

Level B checks that a real order can be created and tracked:

- Call `send_btc` with a valid BTC Lightning invoice, or `receive_btc` with a valid Fiber
  invoice.
- Confirm the response is a `CchOrderResponse`.
- Call `get_cch_order` with the returned `payment_hash`.
- Track status progression or a terminal `Failed` state.

## References

- https://github.com/nervosnetwork/fiber/blob/v0.9.0-rc5/crates/fiber-lib/src/rpc/cch.rs
- https://github.com/nervosnetwork/fiber/blob/v0.9.0-rc5/crates/fiber-json-types/src/cch.rs
- https://github.com/nervosnetwork/fiber/tree/v0.9.0-rc5/crates/fiber-lib/src/cch
- https://github.com/nervosnetwork/fiber/tree/v0.9.0-rc5/tests/bruno/e2e/cross-chain-hub
- https://github.com/nervosnetwork/fiber/tree/v0.9.0-rc5/tests/bruno/e2e/cross-chain-hub-separate
