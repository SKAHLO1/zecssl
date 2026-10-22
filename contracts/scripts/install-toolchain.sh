#!/usr/bin/env bash
# Installs what the NEAR contracts need on top of a stable Rust toolchain.
set -euo pipefail
source "$HOME/.cargo/env"
rustup target add wasm32-unknown-unknown
command -v cargo-near > /dev/null || cargo +1.95.0 install --locked --no-default-features cargo-near@0.22.0  # no Ledger support (needs libudev)
cargo near --version
