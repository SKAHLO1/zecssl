#!/usr/bin/env bash
# Three operators run a 2-of-3 RedPallas DKG through frostd. Each operator gets its own
# config file (its communication keys, contacts and FROST key share). The resulting
# group public key is the treasury's spend-validating key (ak).
set -euo pipefail
source "$(dirname "$0")/env.sh"

mkdir -p "$SPIKE/operators"
if [ -f "$SPIKE/group.key" ]; then
  echo "DKG already done; group key $(group_key)"
  exit 0
fi

for op in "${OPERATORS[@]}"; do
  [ -f "$(cfg "$op")" ] || fc init -c "$(cfg "$op")" > /dev/null
done

# Every operator knows every other operator's communication key.
for op in "${OPERATORS[@]}"; do
  contact="$(fc export -n "$op" -c "$(cfg "$op")" 2>&1 | grep -E '^zffrost' | tail -1)"
  for other in "${OPERATORS[@]}"; do
    [ "$op" = "$other" ] && continue
    fc import -c "$(cfg "$other")" "$contact" > /dev/null 2>&1 || true
  done
done

others="$(pubkey op2),$(pubkey op3)"
echo "Starting DKG: op1 creates the session for op2 ($(pubkey op2 | cut -c1-12)…) and op3 ($(pubkey op3 | cut -c1-12)…)"

fc dkg -c "$(cfg op1)" -d "ZEC SSL treasury (spike)" -s "$FROSTD_ADDR" -C redpallas -t "$THRESHOLD" -S "$others" \
  > "$SPIKE/dkg-op1.log" 2>&1 &
p1=$!
sleep 2
fc dkg -c "$(cfg op2)" -d "ZEC SSL treasury (spike)" -s "$FROSTD_ADDR" -C redpallas -t "$THRESHOLD" \
  > "$SPIKE/dkg-op2.log" 2>&1 &
p2=$!
fc dkg -c "$(cfg op3)" -d "ZEC SSL treasury (spike)" -s "$FROSTD_ADDR" -C redpallas -t "$THRESHOLD" \
  > "$SPIKE/dkg-op3.log" 2>&1 &
p3=$!

status=0
for p in $p1 $p2 $p3; do wait "$p" || status=1; done
if [ "$status" -ne 0 ]; then
  echo "DKG failed; see $SPIKE/dkg-op*.log"
  tail -5 "$SPIKE"/dkg-op*.log
  exit 1
fi

# All three must agree on the group key.
keys=()
for op in "${OPERATORS[@]}"; do
  keys+=("$(fc groups -c "$(cfg "$op")" 2>&1 | grep -oE '[0-9a-f]{64}' | head -1)")
done
if [ "${keys[0]}" != "${keys[1]}" ] || [ "${keys[0]}" != "${keys[2]}" ]; then
  echo "Operators disagree on the group key: ${keys[*]}"
  exit 1
fi
echo "${keys[0]}" > "$SPIKE/group.key"
echo "DKG complete. Group key (ak): ${keys[0]}"
