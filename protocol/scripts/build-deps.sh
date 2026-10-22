#!/usr/bin/env bash
# Builds the pinned upstream tools the signing spike depends on:
#   - ZcashFoundation/frost-tools: frostd (FROST coordination server), frost-client
#     (DKG and signing), zcash-sign (UFVK from a FROST group key)
#   - zcash/zcash-devtool: view-only wallet sync, PCZT create/prove/send
# Checkouts and build output live on the Linux filesystem (~/zec-ssl-deps), which is
# much faster than building under /mnt/c.
set -euo pipefail

DEPS="${ZEC_SSL_DEPS:-$HOME/zec-ssl-deps}"
FROST_TOOLS_REV="53df99c2bd61f7cb60a0d1f634822a76be3799cf" # 2026-10-14
DEVTOOL_REV="0b3436bb8d235c67cd36b800cb45a7b1b489c415"     # 2026-10-17

mkdir -p "$DEPS"
cd "$DEPS"

checkout() {
  local url="$1" dir="$2" rev="$3"
  [ -d "$dir" ] || git clone -q "$url" "$dir"
  git -C "$dir" fetch -q origin
  git -C "$dir" checkout -q "$rev"
}

checkout https://github.com/ZcashFoundation/frost-tools.git frost-tools "$FROST_TOOLS_REV"
checkout https://github.com/zcash/zcash-devtool.git zcash-devtool "$DEVTOOL_REV"

echo "==> frost-tools"
(cd frost-tools && cargo build --release --locked -p frostd -p frost-client -p zcash-sign)

echo "==> zcash-devtool"
(cd zcash-devtool && cargo build --release --locked)

mkdir -p "$DEPS/bin"
cp frost-tools/target/release/{frostd,frost-client,zcash-sign} "$DEPS/bin/"
cp zcash-devtool/target/release/zcash-devtool "$DEPS/bin/"
echo "==> binaries in $DEPS/bin"
ls -la "$DEPS/bin"
