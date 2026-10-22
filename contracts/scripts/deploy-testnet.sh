#!/usr/bin/env bash
# Deploys the test USDC token and the ZEC SSL vault to NEAR testnet and wires them up.
# Run it yourself: it signs with keys from your NEAR CLI keychain.
#
# Requires near-cli-rs (`cargo install near-cli-rs`) and, in the legacy keychain
# (~/.near-credentials), keys for TOKEN_ACCOUNT and VAULT_ACCOUNT. Create testnet accounts with:
#   near account create-account sponsor-by-faucet-service <name>.testnet \
#     autogenerate-new-keypair save-to-legacy-keychain network-config testnet create
#
# Usage:
#   TOKEN_ACCOUNT=usdc.zecssl.testnet VAULT_ACCOUNT=vault.zecssl.testnet \
#   OPERATORS=op1.zecssl.testnet,op2.zecssl.testnet,op3.zecssl.testnet \
#   ARBITERS=arb1.zecssl.testnet,arb2.zecssl.testnet,arb3.zecssl.testnet \
#   bash contracts/scripts/deploy-testnet.sh
set -euo pipefail
cd "$(dirname "$0")/.."

: "${TOKEN_ACCOUNT:?set TOKEN_ACCOUNT}"
: "${VAULT_ACCOUNT:?set VAULT_ACCOUNT}"
: "${OPERATORS:?set OPERATORS (comma-separated)}"
: "${ARBITERS:?set ARBITERS (comma-separated)}"
THRESHOLD="${THRESHOLD:-2}"
ARBITER_THRESHOLD="${ARBITER_THRESHOLD:-2}"

for f in res/zec_ssl_test_token.wasm res/zec_ssl_contract.wasm; do
  [ -f "$f" ] || { echo "missing $f; run scripts/test.sh first"; exit 1; }
done

json_list() { python3 -c 'import json,sys; print(json.dumps([a for a in sys.argv[1].split(",") if a]))' "$1"; }

SEC=1000000000
USDC=1000000
# Testnet parameters: small caps and short windows so failure paths can be exercised
# in minutes. Mainnet values come from the load tests (PRD: open parameters).
INIT_ARGS=$(cat <<EOF
{
  "config": {
    "token": "$TOKEN_ACCOUNT",
    "token_symbol": "USDC",
    "token_decimals": 6,
    "threshold": $THRESHOLD,
    "arbiter_threshold": $ARBITER_THRESHOLD,
    "min_bond": "$((1000 * USDC))",
    "unbonding_delay_ns": "$((7 * 24 * 3600 * SEC))",
    "release_window_ns": "$((30 * 60 * SEC))",
    "dispute_window_ns": "$((24 * 3600 * SEC))",
    "challenge_window_ns": "$((6 * 3600 * SEC))",
    "challenge_bond": "$((10 * USDC))",
    "per_order_cap": "$((1000 * USDC))",
    "epoch_cap": "$((10000 * USDC))",
    "epoch_length_ns": "$((24 * 3600 * SEC))"
  },
  "operators": $(json_list "$OPERATORS"),
  "arbiters": $(json_list "$ARBITERS")
}
EOF
)

echo "==> deploy test token to $TOKEN_ACCOUNT"
near contract deploy "$TOKEN_ACCOUNT" use-file res/zec_ssl_test_token.wasm \
  with-init-call new json-args '{"symbol":"USDC","name":"ZEC SSL test USDC"}' \
  prepaid-gas '30 Tgas' attached-deposit '0 NEAR' \
  network-config testnet sign-with-legacy-keychain send

echo "==> deploy vault to $VAULT_ACCOUNT"
near contract deploy "$VAULT_ACCOUNT" use-file res/zec_ssl_contract.wasm \
  with-init-call new json-args "$INIT_ARGS" \
  prepaid-gas '100 Tgas' attached-deposit '0 NEAR' \
  network-config testnet sign-with-legacy-keychain send

echo "==> register the vault with the token"
near contract call-function as-transaction "$TOKEN_ACCOUNT" storage_deposit \
  json-args "{\"account_id\":\"$VAULT_ACCOUNT\"}" \
  prepaid-gas '30 Tgas' attached-deposit '0.01 NEAR' \
  sign-as "$TOKEN_ACCOUNT" network-config testnet sign-with-legacy-keychain send

cat <<EOF

Deployed. Next:
  1. Each operator mints test USDC and bonds at least 1,000:
       near contract call-function as-transaction $TOKEN_ACCOUNT faucet json-args '{"amount":"10000000000"}' \\
         prepaid-gas '30 Tgas' attached-deposit '0.01 NEAR' sign-as <operator> network-config testnet sign-with-legacy-keychain send
       near contract call-function as-transaction $TOKEN_ACCOUNT ft_transfer_call \\
         json-args '{"receiver_id":"$VAULT_ACCOUNT","amount":"2000000000","msg":"{\\"action\\":\\"bond\\"}"}' \\
         prepaid-gas '100 Tgas' attached-deposit '1 yoctoNEAR' sign-as <operator> network-config testnet sign-with-legacy-keychain send
  2. Add liquidity the same way with msg {"action":"liquidity"}.
  3. Point the API at it: in server/.env set
       NEAR_NETWORK=testnet
       NEAR_VAULT_CONTRACT=$VAULT_ACCOUNT
EOF
