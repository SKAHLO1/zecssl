#!/usr/bin/env bash
# A throwaway testnet "user" wallet for end-to-end tests: the side that deposits shielded
# ZEC into an order, as a real user's wallet would.
#
#   user-wallet.sh init                         create it and print its address
#   user-wallet.sh address                      print its address
#   user-wallet.sh balance
#   user-wallet.sh send <address> <zat> [memo]  pay a deposit address
set -euo pipefail
DEPS="${ZEC_SSL_DEPS:-$HOME/zec-ssl-deps}"
W="${USER_WALLET:-$HOME/zec-ssl-run/user-wallet}"
ID="$W.age"
devtool() { "$DEPS/bin/zcash-devtool" "$@"; }

addr() { devtool wallet -w "$W" list-addresses 2>/dev/null | grep -oE 'utest1[0-9a-z]+' | head -1; }

case "${1:-}" in
  init)
    [ -d "$W" ] || devtool wallet -w "$W" init --name user -i "$ID" -n test -s zecrocks > /dev/null
    addr
    ;;
  address) addr ;;
  balance)
    devtool wallet -w "$W" sync -s zecrocks > /dev/null 2>&1
    devtool wallet -w "$W" balance | grep -E 'Height|TAZ'
    ;;
  send)
    [ $# -ge 3 ] || { echo "usage: $0 send <address> <zat> [memo]"; exit 2; }
    devtool wallet -w "$W" sync -s zecrocks > /dev/null 2>&1
    devtool wallet -w "$W" send -i "$ID" --address "$2" --value "$3" ${4:+--memo "$4"} -s zecrocks
    ;;
  *) echo "usage: $0 init|address|balance|send"; exit 2 ;;
esac
