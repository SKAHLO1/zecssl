#!/usr/bin/env bash
# Builds the coordinator and operator in WSL and uploads them to a server too small to
# compile them (e.g. an AWS t3.small). The server then builds protocol/Dockerfile.prebuilt
# (set PROTOCOL_DOCKERFILE=Dockerfile.prebuilt in deploy/.env).
#
#   HOST=3.85.120.7 bash deploy/push-binaries.sh
#
# The server must have the same CPU architecture as this machine (x86_64) and Ubuntu 24.04
# images, so the binaries' glibc matches.
set -euo pipefail
: "${HOST:?set HOST to the server address}"
USER_NAME="${SSH_USER:-ubuntu}"
KEY_SRC="${SSH_KEY:-/mnt/c/Users/USER/.ssh/oracle_zec_ssl}"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="${CARGO_TARGET_DIR:-$HOME/zec-ssl-deps/target-protocol}"

# ssh refuses keys readable by others, and files under /mnt/c always look that way.
KEY="$HOME/.ssh/zec-ssl-deploy"
mkdir -p "$HOME/.ssh"
install -m 600 "$KEY_SRC" "$KEY"

source "$HOME/.cargo/env"
(cd "$REPO/protocol" && CARGO_TARGET_DIR="$TARGET" cargo build --release --locked -p ssl-node)

ssh -i "$KEY" -o StrictHostKeyChecking=accept-new "$USER_NAME@$HOST" "mkdir -p zec-ssl/protocol/bin"
scp -i "$KEY" "$TARGET/release/ssl-coordinator" "$TARGET/release/ssl-operator" "$USER_NAME@$HOST:zec-ssl/protocol/bin/"
echo "uploaded to $HOST:zec-ssl/protocol/bin. On the server: cd ~/zec-ssl/deploy && docker compose up -d --build"
