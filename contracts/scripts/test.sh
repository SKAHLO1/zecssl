#!/usr/bin/env bash
# Unit tests, wasm builds, then sandbox integration tests. Run inside WSL/Linux.
set -euo pipefail
source "$HOME/.cargo/env"
cd "$(dirname "$0")/.."
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/zec-ssl-deps/target-contracts}"

echo "==> unit tests"
cargo test -p zec-ssl-contract --lib

echo "==> wasm builds"
mkdir -p res
for pkg in ssl test-token; do
  (cd "$pkg" && cargo near build non-reproducible-wasm --no-abi)
done
cp "$CARGO_TARGET_DIR"/near/zec_ssl_contract/zec_ssl_contract.wasm res/
cp "$CARGO_TARGET_DIR"/near/zec_ssl_test_token/zec_ssl_test_token.wasm res/
ls -la res

echo "==> sandbox integration tests"
cargo test -p zec-ssl-contract --test sandbox -- --nocapture
