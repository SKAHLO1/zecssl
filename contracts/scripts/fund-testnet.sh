#!/usr/bin/env bash
# After deploy-testnet.sh: each operator mints test USDC from the faucet and bonds it,
# then the first operator adds vault liquidity. Signs with your NEAR CLI legacy keychain.
#
#   TOKEN_ACCOUNT=... VAULT_ACCOUNT=... OPERATORS=a.testnet,b.testnet,c.testnet \
#   BOND=2000 LIQUIDITY=5000 bash contracts/scripts/fund-testnet.sh
set -euo pipefail

: "${TOKEN_ACCOUNT:?set TOKEN_ACCOUNT}"
: "${VAULT_ACCOUNT:?set VAULT_ACCOUNT}"
: "${OPERATORS:?set OPERATORS (comma-separated)}"
BOND="${BOND:-2000}"
LIQUIDITY="${LIQUIDITY:-5000}"
USDC=1000000

call() { # <signer> <method> <json-args> <gas> <deposit>
  near contract call-function as-transaction "$TOKEN_ACCOUNT" "$2" json-args "$3" \
    prepaid-gas "$4" attached-deposit "$5" \
    sign-as "$1" network-config testnet sign-with-legacy-keychain send
}

IFS=, read -r -a ops <<< "$OPERATORS"
for op in "${ops[@]}"; do
  mint=$((BOND * USDC))
  [ "$op" = "${ops[0]}" ] && mint=$(((BOND + LIQUIDITY) * USDC))
  echo "==> $op: mint $((mint / USDC)) USDC, bond $BOND"
  call "$op" faucet "{\"amount\":\"$mint\"}" '30 Tgas' '0.01 NEAR'
  call "$op" ft_transfer_call \
    "{\"receiver_id\":\"$VAULT_ACCOUNT\",\"amount\":\"$((BOND * USDC))\",\"msg\":\"{\\\"action\\\":\\\"bond\\\"}\"}" \
    '100 Tgas' '1 yoctoNEAR'
done

echo "==> ${ops[0]}: add $LIQUIDITY USDC liquidity"
call "${ops[0]}" ft_transfer_call \
  "{\"receiver_id\":\"$VAULT_ACCOUNT\",\"amount\":\"$((LIQUIDITY * USDC))\",\"msg\":\"{\\\"action\\\":\\\"liquidity\\\"}\"}" \
  '100 Tgas' '1 yoctoNEAR'

echo "==> vault books"
near contract call-function as-read-only "$VAULT_ACCOUNT" get_accounting json-args '{}' network-config testnet now
