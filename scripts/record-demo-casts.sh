#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ -z "${FIBER_BIN:-}" ]]; then
  if command -v fiber >/dev/null 2>&1; then
    FIBER_BIN="fiber"
  else
    FIBER_BIN="target/debug/fiber"
  fi
fi
FIBER_DISPLAY="${FIBER_DISPLAY:-$FIBER_BIN}"

section() {
  printf '\n'
  printf '================================================================================\n'
  printf '%s\n' "$1"
  printf '================================================================================\n'
}

run() {
  print_command "$@"
  "$@"
}

print_command() {
  printf '\n$'
  local first=1
  local shown
  for arg in "$@"; do
    shown="$arg"
    if [[ "$first" -eq 1 && "$arg" == "$FIBER_BIN" ]]; then
      shown="$FIBER_DISPLAY"
    fi
    printf ' %q' "$shown"
    first=0
  done
  printf '\n'
}

pause() {
  printf '\n%s\n' "$1"
  sleep 1
}

fiber_bin_path() {
  if [[ "$FIBER_BIN" == */* ]]; then
    printf '%s\n' "$ROOT_DIR/$FIBER_BIN"
  else
    command -v "$FIBER_BIN"
  fi
}

run_expected_failure_to_log() {
  local log_path="$1"
  shift

  print_command "$@"
  printf '  # output -> %s\n' "$log_path"

  set +e
  "$@" >"$log_path" 2>&1
  local status=$?
  set -e

  cat "$log_path"
  printf '\nexpected failure exit status: %s\n' "$status"
}

run_to_log() {
  local log_path="$1"
  shift

  print_command "$@"
  printf '  # output -> %s\n' "$log_path"
  "$@" >"$log_path"
  printf 'wrote structured output to %s\n' "$log_path"
}

run_fiber_up() {
  print_command "$FIBER_BIN" up

  set +e
  "$FIBER_BIN" up
  local status=$?
  set -e

  if [[ "$status" -eq 0 ]]; then
    return 0
  fi

  printf '\nFirst startup attempt failed; retrying once after cleanup.\n'
  print_command "$FIBER_BIN" down
  "$FIBER_BIN" down

  print_command "$FIBER_BIN" up
  "$FIBER_BIN" up
}

cleanup() {
  "$FIBER_BIN" down >/dev/null 2>&1 || true
}

trap cleanup EXIT

case "${1:-}" in
  network)
    section "Fiber DevKit: local network startup and visibility"
    run "$FIBER_BIN" reset
    run_fiber_up
    run "$FIBER_BIN" validate --live
    run "$FIBER_BIN" inspect
    run_to_log /tmp/fiber-network-smoke-demo.jsonl \
      "$FIBER_BIN" run scenarios/network-smoke.yaml --report
    run "$FIBER_BIN" report --format md
    ;;

  basic-payment)
    section "Fiber DevKit: funded happy-path payment"
    run "$FIBER_BIN" reset
    run pnpm balances:nodes
    run pnpm fund:nodes
    run_fiber_up
    run "$FIBER_BIN" run scenarios/basic-payment.yaml --report
    run "$FIBER_BIN" inspect node-1 --channels
    run "$FIBER_BIN" report --format md
    ;;

  low-liquidity)
    section "Fiber DevKit: structured failure diagnosis"
    run "$FIBER_BIN" reset
    run pnpm balances:nodes
    run pnpm fund:nodes
    run_fiber_up
    run_expected_failure_to_log /tmp/fiber-low-liquidity-demo.jsonl \
      "$FIBER_BIN" run scenarios/low-liquidity.yaml
    run "$FIBER_BIN" doctor /tmp/fiber-low-liquidity-demo.jsonl --explain
    ;;

  predict)
    section "Fiber DevKit: route prediction and CCH honesty"
    run "$FIBER_BIN" reset
    run pnpm balances:nodes
    run pnpm fund:nodes
    run_fiber_up
    run "$FIBER_BIN" run scenarios/basic-payment.yaml --report
    run "$FIBER_BIN" predict node-1 node-2 1
    run "$FIBER_BIN" predict node-1 node-2 1 --cross-chain
    run "$FIBER_BIN" simulate node-1 node-2 1 --dry-run
    ;;

  doctor)
    section "Fiber DevKit: taxonomy explanations"
    run "$FIBER_BIN" --help
    pause "Next: explain a liquidity taxonomy code."
    run "$FIBER_BIN" doctor FIBER_LIQ_001 --explain
    pause "Next: classify a CCH invoice validation error."
    run "$FIBER_BIN" doctor "CCH gateway rejected btc_pay_req: invoice amount mismatch" --explain
    pause "Next: classify a connectivity failure."
    run "$FIBER_BIN" doctor "connection refused while calling node_info" --explain
    pause "Next: classify a route-building failure."
    run "$FIBER_BIN" doctor "no route to target" --explain
    ;;

  report-ci)
    section "Fiber DevKit: report artifacts and CI scaffold"
    run "$FIBER_BIN" reset
    run_fiber_up
    run_to_log /tmp/fiber-report-ci-smoke.jsonl \
      "$FIBER_BIN" run scenarios/network-smoke.yaml --report
    run "$FIBER_BIN" report --format md
    run "$FIBER_BIN" report --format json
    tmp_dir="$(mktemp -d)"
    ci_bin="$(fiber_bin_path)"
    printf '\n$ mkdir -p %q && cd %q && %q ci init\n' "$tmp_dir" "$tmp_dir" "$FIBER_DISPLAY"
    (cd "$tmp_dir" && "$ci_bin" ci init)
    printf '\n$ sed -n %q %q\n' '1,90p' "$tmp_dir/.github/workflows/fiber-ci.yml"
    sed -n '1,90p' "$tmp_dir/.github/workflows/fiber-ci.yml"
    ;;

  *)
    cat <<'USAGE'
Usage:
  bash scripts/record-demo-casts.sh network
  bash scripts/record-demo-casts.sh basic-payment
  bash scripts/record-demo-casts.sh low-liquidity
  bash scripts/record-demo-casts.sh predict
  bash scripts/record-demo-casts.sh doctor
  bash scripts/record-demo-casts.sh report-ci
USAGE
    exit 2
    ;;
esac
