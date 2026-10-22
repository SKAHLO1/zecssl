#!/usr/bin/env bash
# Verifies every broadcast spend in $SPIKE/runs:
#   1. Extracts the final transaction from the signed PCZT (re-verifying all proofs and
#      signatures) and checks it has no transparent inputs or outputs.
#   2. Checks its txid equals the one the network accepted. ZIP 244 txids commit to all
#      effecting data, transparent inputs and outputs included, so a match means the
#      checked transaction is the one on chain.
#   3. Looks the txid up through lightwalletd to confirm it was mined.
set -uo pipefail
source "$(dirname "$0")/env.sh"

SSL_SIGN="${SSL_SIGN:-$BIN/ssl-sign}"
fail=0; n=0
printf "%-10s %-66s %-9s %-12s %s\n" PAIR TXID SHIELDED BLOCK RESULT
for run in "$SPIKE"/runs/*; do
  [ -f "$run/signed.pczt" ] && [ -f "$run/send.log" ] || continue
  n=$((n + 1))
  pair="$(basename "$run" | sed -E 's/.*Z-//; s/-/+/')"
  sent="$(grep -oE '^[0-9a-f]{64}$' "$run/send.log" | tail -1)"
  json="$("$SSL_SIGN" extract "$run/signed.pczt")" || { echo "$pair extraction failed"; fail=1; continue; }
  read -r txid shielded < <(python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["txid"], d["fully_shielded"])' <<< "$json")
  block="$(timeout 60 "$BIN/zcash-devtool" inspect --lookup "$txid" < /dev/null 2>&1 | grep -oE 'block [0-9]+' | grep -oE '[0-9]+' || true)"
  result="ok"
  [ "$txid" = "$sent" ] || result="txid mismatch (sent $sent)"
  [ "$shielded" = "True" ] || result="TRANSPARENT I/O"
  [ -n "$block" ] || result="${result/ok/not mined yet}"
  [ "$result" = "ok" ] || fail=1
  printf "%-10s %-66s %-9s %-12s %s\n" "$pair" "$txid" "$shielded" "${block:--}" "$result"
done
[ "$n" -gt 0 ] || { echo "no runs found"; exit 1; }
exit "$fail"
