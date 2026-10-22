#!/usr/bin/env bash
# Spends from the treasury with exactly two of the three operators.
#
#   04-spend.sh <signer-a> <signer-b> <to-address> <zatoshis>
#
# 1. devtool builds and proves a PCZT from the view-only treasury wallet.
# 2. ssl-sign extracts the sighash and each spend's randomizer, refusing any
#    transaction with a transparent input or output.
# 3. The two signers run a rerandomized FROST round through frostd; the third operator
#    coordinates without contributing a share.
# 4. ssl-sign applies the signatures (each is verified against the spend's rk), and
#    devtool broadcasts the transaction.
set -euo pipefail
source "$(dirname "$0")/env.sh"

[ $# -eq 4 ] || { echo "usage: $0 <signer-a> <signer-b> <to-address> <zatoshis>"; exit 2; }
A="$1"; B="$2"; TO="$3"; VALUE="$4"
COORD=""
for op in "${OPERATORS[@]}"; do [ "$op" != "$A" ] && [ "$op" != "$B" ] && COORD="$op"; done
[ -n "$COORD" ] || { echo "signers must be two distinct operators from: ${OPERATORS[*]}"; exit 2; }

W="$SPIKE/wallet"
RUN="$SPIKE/runs/$(date -u +%Y%m%dT%H%M%SZ)-$A-$B"
mkdir -p "$RUN/sign"
echo "Run directory: $RUN"

# frostd lives only as long as this script.
bash "$(dirname "$0")/01-frostd.sh"
trap 'kill "$(cat "$SPIKE/frostd.pid")" 2>/dev/null || true' EXIT

echo "==> sync"
devtool wallet -w "$W" sync -s "$LWD" > "$RUN/sync.log" 2>&1

echo "==> create PCZT: $VALUE zat to $TO"
devtool pczt -w "$W" create --address "$TO" --value "$VALUE" --memo "zec-ssl spike: $A+$B" --output "$RUN/unsigned.pczt"

echo "==> prove"
devtool pczt -w "$W" prove "$RUN/unsigned.pczt" --output "$RUN/proven.pczt"

echo "==> prepare signing request"
"$BIN/ssl-sign" prepare "$RUN/proven.pczt" --require-shielded --out-dir "$RUN/sign" > "$RUN/request.json"
cat "$RUN/request.json"

msgs=(); rands=(); sigs=(); ids=()
while read -r pool index; do
  msgs+=(-m "$RUN/sign/sighash.bin")
  rands+=(-r "$RUN/sign/$pool-$index.alpha.bin")
  sigs+=(-o "$RUN/sign/$pool-$index.sig")
  ids+=("$pool $index")
done < <(python3 -c 'import json,sys; [print(s["pool"], s["index"]) for s in json.load(open(sys.argv[1]))["spends"]]' "$RUN/request.json")
[ ${#ids[@]} -gt 0 ] || { echo "no spends awaiting signature"; exit 1; }

echo "==> FROST round: $A + $B sign, $COORD coordinates (${#ids[@]} spend(s))"
fc coordinator -c "$(cfg "$COORD")" -g "$(group_key)" -s "$FROSTD_ADDR" \
  -S "$(pubkey "$A"),$(pubkey "$B")" "${msgs[@]}" "${rands[@]}" "${sigs[@]}" \
  > "$RUN/coordinator.log" 2>&1 &
pc=$!
sleep 3
for op in "$A" "$B"; do
  yes y | fc participant -c "$(cfg "$op")" -g "$(group_key)" -s "$FROSTD_ADDR" > "$RUN/participant-$op.log" 2>&1 &
  eval "p_$op=\$!"
done
status=0
wait "$pc" || status=1
for op in "$A" "$B"; do eval "wait \$p_$op" || status=1; done
if [ "$status" -ne 0 ]; then
  echo "FROST round failed; logs in $RUN"
  tail -5 "$RUN"/coordinator.log "$RUN"/participant-*.log
  exit 1
fi

python3 - "$RUN" "${ids[@]}" <<'EOF'
import json, sys
run, ids = sys.argv[1], sys.argv[2:]
out = []
for entry in ids:
    pool, index = entry.split()
    sig = open(f"{run}/sign/{pool}-{index}.sig", "rb").read()
    out.append({"pool": pool, "index": int(index), "signature": sig.hex()})
json.dump(out, open(f"{run}/signatures.json", "w"), indent=2)
EOF

echo "==> apply signatures"
"$BIN/ssl-sign" apply --pczt "$RUN/proven.pczt" --signatures "$RUN/signatures.json" --out "$RUN/signed.pczt"

echo "==> broadcast"
devtool pczt -w "$W" send "$RUN/signed.pczt" -s "$LWD" | tee "$RUN/send.log"
