#!/usr/bin/env bash
# Runs three operator nodes and the coordinator on this machine against a NEAR testnet
# vault and the Zcash testnet. Every process gets its own state directory and wallet; the
# operators sign with their own keys from the NEAR CLI keychain. Ctrl-C stops them all.
#
#   VAULT=zecssl-skahlo-vault.testnet PREFIX=zecssl-skahlo bash protocol/run/start-local.sh
#
# Settings (environment):
#   VAULT        the vault contract account (required)
#   PREFIX       operator accounts are $PREFIX-op1.testnet ... -op3 (required)
#   UFVK_FILE    treasury viewing key file (default: the Phase 2 spike treasury)
#   BIRTHDAY     wallet birthday height (default: 4524400, the spike treasury's)
#   CONFIRMATIONS                (default: 10)
#   STATE        state root (default: ~/zec-ssl-run)
set -euo pipefail

: "${VAULT:?set VAULT}"
: "${PREFIX:?set PREFIX}"
DEPS="${ZEC_SSL_DEPS:-$HOME/zec-ssl-deps}"
BIN="${BIN:-$DEPS/target-protocol/release}"
STATE="${STATE:-$HOME/zec-ssl-run}"
UFVK_FILE="${UFVK_FILE:-$HOME/zec-ssl-spike/ufvk.txt}"
CREDS="$HOME/.near-credentials/testnet"
mkdir -p "$STATE"
chmod 700 "$STATE"

# A random token the coordinator presents to operators, generated once and kept locally.
TOKEN_FILE="$STATE/operator-api-token"
[ -f "$TOKEN_FILE" ] || { head -c 32 /dev/urandom | xxd -p -c 64 > "$TOKEN_FILE"; chmod 600 "$TOKEN_FILE"; }

common=(
  NEAR_RPC_URL=https://rpc.testnet.fastnear.com
  NEAR_VAULT_CONTRACT="$VAULT"
  ZCASH_NETWORK=test
  LIGHTWALLETD_URL=https://testnet.zec.rocks:443
  TREASURY_UFVK="@$UFVK_FILE"
  WALLET_BIRTHDAY="${BIRTHDAY:-4524400}"
  CONFIRMATIONS="${CONFIRMATIONS:-10}"
  OPERATOR_API_TOKEN="$(cat "$TOKEN_FILE")"
  RUST_LOG="${RUST_LOG:-info}"
)

pids=()
cleanup() { kill "${pids[@]}" 2>/dev/null || true; wait 2>/dev/null || true; }
trap cleanup EXIT INT TERM

for i in 1 2 3; do
  account="$PREFIX-op$i.testnet"
  [ -f "$CREDS/$account.json" ] || { echo "missing key for $account in $CREDS"; exit 1; }
  env "${common[@]}" \
    OPERATOR_LISTEN="127.0.0.1:710$i" \
    NEAR_CREDENTIALS="$CREDS/$account.json" \
    STATE_DIR="$STATE/op$i" \
    "$BIN/ssl-operator" > "$STATE/op$i.log" 2>&1 &
  pids+=($!)
  echo "operator $i ($account) on :710$i, log $STATE/op$i.log"
done

env "${common[@]}" \
  COORDINATOR_LISTEN=127.0.0.1:7000 \
  OPERATORS="$PREFIX-op1.testnet=http://127.0.0.1:7101,$PREFIX-op2.testnet=http://127.0.0.1:7102,$PREFIX-op3.testnet=http://127.0.0.1:7103" \
  STATE_DIR="$STATE/coordinator" \
  "$BIN/ssl-coordinator" > "$STATE/coordinator.log" 2>&1 &
pids+=($!)
echo "coordinator on :7000, log $STATE/coordinator.log"
echo
echo "For the website's API (server/.env):"
echo "  COORDINATOR_URL=http://127.0.0.1:7000"
echo "  OPERATOR_ENDPOINTS=$PREFIX-op1.testnet=http://127.0.0.1:7101,$PREFIX-op2.testnet=http://127.0.0.1:7102,$PREFIX-op3.testnet=http://127.0.0.1:7103"

# One process exiting must not take the others down: the network is built to keep
# settling with up to n - t operators offline. Report exits and keep running until
# Ctrl-C or every process has exited.
remaining=${#pids[@]}
while [ "$remaining" -gt 0 ]; do
  if wait -n "${pids[@]}" 2>/dev/null; then status=0; else status=$?; fi
  remaining=$((remaining - 1))
  echo "$(date -u +%H:%M:%S) a process exited (status $status); $remaining still running"
done
