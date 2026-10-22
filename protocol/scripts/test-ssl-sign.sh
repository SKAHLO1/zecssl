#!/usr/bin/env bash
# Builds and tests the protocol workspace. Run inside WSL/Linux.
set -euo pipefail
HERE="$(cd "$(dirname "$0")/.." && pwd)"
DEPS="${ZEC_SSL_DEPS:-$HOME/zec-ssl-deps}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$DEPS/target-protocol}"

FIXTURE="$HERE/ssl-sign/tests/fixtures/ironwood_v6.pczt"
if [ ! -f "$FIXTURE" ]; then
  mkdir -p "$(dirname "$FIXTURE")"
  cp "$DEPS/frost-tools/zcash-sign/tests/fixtures/ironwood_v6.pczt" "$FIXTURE"
fi

cd "$HERE"
cargo test --release -p ssl-sign "$@"
cargo build --release -p ssl-sign
mkdir -p "$DEPS/bin"
cp "$CARGO_TARGET_DIR/release/ssl-sign" "$DEPS/bin/"
