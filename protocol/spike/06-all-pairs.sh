#!/usr/bin/env bash
# Runs a treasury spend for each listed operator pair in turn, waiting between spends
# until the previous change note is spendable. Payments go back to the treasury address
# so the test funds are reused.
#
#   06-all-pairs.sh [zatoshis] [pair ...]     e.g. 06-all-pairs.sh 1000000 op2:op3 op1:op3
set -euo pipefail
source "$(dirname "$0")/env.sh"

VALUE="${1:-1000000}"
[ $# -gt 0 ] && shift
if [ $# -gt 0 ]; then PAIRS=("$@"); else PAIRS=(op1:op2 op2:op3 op1:op3); fi
W="$SPIKE/wallet"
TO="$(cat "$SPIKE/address.txt")"

spendable() {
  # Shielded only: the treasury never spends transparent funds.
  devtool wallet -w "$W" balance --json 2>/dev/null | python3 -c '
import json, sys
d = json.loads([l for l in sys.stdin if l.strip().startswith("{")][-1])
print(d["ironwood_spendable"] + d["orchard_spendable"])'
}

for pair in "${PAIRS[@]}"; do
  A="${pair%%:*}"; B="${pair##*:}"
  need=$((VALUE + 20000))
  for i in $(seq 1 60); do
    devtool wallet -w "$W" sync -s "$LWD" > /dev/null 2>&1 || true
    have="$(spendable || echo 0)"
    [ "$have" -ge "$need" ] && break
    echo "$(date -u +%H:%M:%S) waiting for spendable funds: $have/$need zat"
    sleep 30
  done
  echo "=== $A + $B"
  bash "$(dirname "$0")/04-spend.sh" "$A" "$B" "$TO" "$VALUE" 2>&1 | grep -E '^Run directory|^==> FROST|^[0-9a-f]{64}$|refusing|failed|Error|error'
done
