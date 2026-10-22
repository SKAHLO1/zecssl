#!/usr/bin/env bash
# Offline check of the signing half, no funds needed: every operator pair runs a
# rerandomized FROST round over a real v6 sighash and randomizer (taken from the
# ssl-sign test fixture), and each signature is verified against our group key
# rerandomized by that alpha. Proves DKG output + frostd + frost-client produce valid
# RedPallas spend-authorization signatures for this treasury.
set -euo pipefail
source "$(dirname "$0")/env.sh"
FIXTURE="$(cd "$(dirname "$0")/.." && pwd)/ssl-sign/tests/fixtures/ironwood_v6.pczt"

RUN="$SPIKE/selftest"
rm -rf "$RUN"; mkdir -p "$RUN"
"$BIN/ssl-sign" prepare "$FIXTURE" --out-dir "$RUN" > "$RUN/request.json"
read -r POOL INDEX ALPHA SIGHASH < <(python3 -c '
import json,sys; r=json.load(open(sys.argv[1])); s=r["spends"][0]
print(s["pool"], s["index"], s["alpha"], r["sighash"])' "$RUN/request.json")

bash "$(dirname "$0")/01-frostd.sh" > /dev/null
trap 'kill "$(cat "$SPIKE/frostd.pid")" 2>/dev/null || true' EXIT

pass=0
for pair in "op1 op2" "op2 op3" "op1 op3"; do
  set -- $pair; A=$1; B=$2
  for op in "${OPERATORS[@]}"; do [ "$op" != "$A" ] && [ "$op" != "$B" ] && COORD="$op"; done
  out="$RUN/$A-$B.sig"
  fc coordinator -c "$(cfg "$COORD")" -g "$(group_key)" -s "$FROSTD_ADDR" -S "$(pubkey "$A"),$(pubkey "$B")" \
    -m "$RUN/sighash.bin" -r "$RUN/$POOL-$INDEX.alpha.bin" -o "$out" > "$RUN/$A-$B.coord.log" 2>&1 &
  pc=$!
  sleep 3
  yes y | fc participant -c "$(cfg "$A")" -g "$(group_key)" -s "$FROSTD_ADDR" > "$RUN/$A-$B.$A.log" 2>&1 &
  pa=$!
  yes y | fc participant -c "$(cfg "$B")" -g "$(group_key)" -s "$FROSTD_ADDR" > "$RUN/$A-$B.$B.log" 2>&1 &
  pb=$!
  if wait $pc && wait $pa && wait $pb && [ -s "$out" ]; then
    sig="$(xxd -p -c 64 "$out")"
    tampered="$(printf '%s' "$SIGHASH" | sed 's/^./0/;t;s/^./1/')"
    [ "$tampered" = "$SIGHASH" ] && tampered="1${SIGHASH:1}"
    if "$BIN/ssl-sign" verify --ak "$(group_key)" --alpha "$ALPHA" --sighash "$tampered" --signature "$sig" > /dev/null 2>&1; then
      echo "FAIL  $A+$B signature also verifies over a tampered sighash; the check is broken"
    elif "$BIN/ssl-sign" verify --ak "$(group_key)" --alpha "$ALPHA" --sighash "$SIGHASH" --signature "$sig" > /dev/null; then
      echo "PASS  $A+$B signed, $COORD coordinated: ${sig:0:16}…"
      pass=$((pass + 1))
    else
      echo "FAIL  $A+$B produced a signature that does not verify"
    fi
  else
    echo "FAIL  $A+$B round did not complete; logs in $RUN"
    tail -3 "$RUN/$A-$B".*.log
  fi
done
echo "$pass/3 operator pairs produced valid rerandomized signatures"
[ "$pass" -eq 3 ]
