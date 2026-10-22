#!/usr/bin/env bash
# Starts a local frostd with TLS from a throwaway CA.
set -euo pipefail
source "$(dirname "$0")/env.sh"

mkdir -p "$SPIKE/tls"
cd "$SPIKE/tls"
if [ ! -f server.pem ]; then
  openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 30 \
    -subj "/CN=zec-ssl spike CA" -keyout ca-key.pem -out ca.pem 2>/dev/null
  openssl req -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes \
    -subj "/CN=localhost" -keyout server-key.pem -out server.csr 2>/dev/null
  printf "subjectAltName=DNS:localhost,IP:127.0.0.1\nbasicConstraints=CA:FALSE\n" > ext.cnf
  openssl x509 -req -in server.csr -CA ca.pem -CAkey ca-key.pem -CAcreateserial \
    -days 30 -extfile ext.cnf -out server.pem 2>/dev/null
fi

if [ -f "$SPIKE/frostd.pid" ] && kill -0 "$(cat "$SPIKE/frostd.pid")" 2>/dev/null; then
  echo "frostd already running (pid $(cat "$SPIKE/frostd.pid"))"
  exit 0
fi

"$BIN/frostd" --ip 127.0.0.1 --port 2744 --tls-cert server.pem --tls-key server-key.pem \
  > "$SPIKE/frostd.log" 2>&1 &
echo $! > "$SPIKE/frostd.pid"
sleep 1
kill -0 "$(cat "$SPIKE/frostd.pid")" && echo "frostd listening on https://$FROSTD_ADDR (pid $(cat "$SPIKE/frostd.pid"))"
