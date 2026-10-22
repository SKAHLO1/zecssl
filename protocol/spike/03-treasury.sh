#!/usr/bin/env bash
# Derives the treasury's unified full viewing key and address from the FROST group key,
# then creates a view-only wallet for it and syncs it from testnet lightwalletd.
#
# nk and rivk are generated randomly (they grant viewing, not spending); ak is the FROST
# group key. NOTE: this uses the key derivation that is not compatible with ZIP 2005
# quantum recoverability, as upstream frost-tools does today. See README.
set -euo pipefail
source "$(dirname "$0")/env.sh"

W="$SPIKE/wallet"
# Generation is randomized (nk and rivk), so a second run would yield a different viewing
# key and address for the same group key. Never regenerate once a wallet exists.
if [ ! -f "$SPIKE/ufvk.txt" ] && [ -d "$W" ]; then
  echo "A wallet exists but $SPIKE/ufvk.txt is missing; refusing to generate a different viewing key."
  exit 1
fi
if [ ! -f "$SPIKE/ufvk.txt" ]; then
  out="$("$BIN/zcash-sign" generate --ak "$(group_key)" --network "$ZNET")"
  echo "$out" | grep -m1 'Unified Full Viewing Key' | sed -E 's/.*"(.*)".*/\1/' > "$SPIKE/ufvk.txt"
  echo "$out" | grep -m1 'unified address' | sed -E 's/.*"(.*)".*/\1/' > "$SPIKE/address.txt"
  chmod 600 "$SPIKE/ufvk.txt"
fi

if [ ! -d "$W" ]; then
  # Birthday: a little before the current tip, so the first sync is short.
  devtool wallet -w "$W" init-fvk --name treasury --fvk "$(cat "$SPIKE/ufvk.txt")" -s "$LWD" ${BIRTHDAY:+--birthday "$BIRTHDAY"}
fi

devtool wallet -w "$W" sync -s "$LWD"
devtool wallet -w "$W" balance

echo
echo "Treasury address (fund this with testnet ZEC):"
cat "$SPIKE/address.txt"
