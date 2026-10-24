#!/usr/bin/env bash
# Writes deploy/.env from the keys on this machine (run in WSL, where they live):
# the operators' NEAR keys, their FROST shares, the treasury viewing key and the faucet key.
#
#   PREFIX=zecssl-skahlo API_DOMAIN=1-2-3-4.sslip.io WEB_ORIGIN=https://zec-ssl.vercel.app bash deploy/make-env.sh
#
# The file holds every secret of the deployment. Copy it to the server with scp, keep it
# out of git (it is ignored), and delete local copies you don't need.
set -euo pipefail
: "${PREFIX:?set PREFIX, e.g. zecssl-skahlo}"
: "${API_DOMAIN:?set API_DOMAIN, e.g. 1-2-3-4.sslip.io for server IP 1.2.3.4}"
: "${WEB_ORIGIN:?set WEB_ORIGIN, the website URL, e.g. https://zec-ssl.vercel.app}"
CREDS="${CREDS:-$HOME/.near-credentials/testnet}"
SPIKE="${SPIKE:-$HOME/zec-ssl-spike}"
OUT="$(dirname "$0")/.env"

key() { python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["private_key"])' "$CREDS/$1.json"; }
# frost-client keeps each share as hex under [group.<group key>]; ssl-frost reads the same hex.
frost() {
  python3 - "$SPIKE/operators/$1.toml" "$(cat "$SPIKE/group.key")" "$2" <<'PY'
import sys, tomllib
cfg = tomllib.load(open(sys.argv[1], "rb"))
print(cfg["group"][sys.argv[2]][sys.argv[3]])
PY
}

umask 077
{
  echo "NEAR_VAULT_CONTRACT=$PREFIX-vault.testnet"
  echo "TREASURY_UFVK=$(tr -d '\n' < "$SPIKE/ufvk.txt")"
  echo "WALLET_BIRTHDAY=${BIRTHDAY:-4524400}"
  echo "OPERATOR_API_TOKEN=$(head -c 32 /dev/urandom | xxd -p -c 64)"
  echo "FROST_PUBLIC_KEY_PACKAGE=$(frost op1 public_key_package)"
  for i in 1 2 3; do
    echo "OP${i}_ACCOUNT=$PREFIX-op$i.testnet"
    echo "OP${i}_NEAR_KEY=$(key "$PREFIX-op$i.testnet")"
    echo "OP${i}_FROST_KEY_PACKAGE=$(frost "op$i" key_package)"
  done
  # Set PREBUILT=1 for servers that run binaries uploaded by push-binaries.sh.
  [ -n "${PREBUILT:-}" ] && echo "PROTOCOL_DOCKERFILE=Dockerfile.prebuilt"
  echo "API_DOMAIN=$API_DOMAIN"
  echo "CORS_ORIGINS=$WEB_ORIGIN"
  if [ -f "$CREDS/$PREFIX-usdc.testnet.json" ]; then
    echo "NEAR_FAUCET_ACCOUNT=$PREFIX-usdc.testnet"
    echo "NEAR_FAUCET_KEY=$(key "$PREFIX-usdc.testnet")"
  fi
} > "$OUT"
echo "wrote $OUT ($(wc -l < "$OUT") settings). It contains secrets."
