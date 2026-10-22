# Shared settings for the signing spike. Source this file; do not run it.
# Everything stateful (keys, wallet, PCZTs) lives in $SPIKE on the Linux filesystem,
# never in the repository.

export DEPS="${ZEC_SSL_DEPS:-$HOME/zec-ssl-deps}"
export BIN="$DEPS/bin"
export SPIKE="${ZEC_SSL_SPIKE:-$HOME/zec-ssl-spike}"

# Zcash testnet, via the zec.rocks testnet lightwalletd (testnet.zec.rocks:443).
export ZNET="test"
export LWD="zecrocks"

# Local FROST coordination server. TLS uses a throwaway CA created by 01-frostd.sh;
# frost-client trusts it through SSL_CERT_FILE, so no system trust store is touched.
export FROSTD_ADDR="127.0.0.1:2744"
export SSL_CERT_FILE="$SPIKE/tls/ca.pem"

export OPERATORS=(op1 op2 op3)
export THRESHOLD=2

cfg() { echo "$SPIKE/operators/$1.toml"; }
fc() { "$BIN/frost-client" "$@"; }
devtool() { "$BIN/zcash-devtool" "$@"; }

# The operator's communication public key, as frost-client stores it in its config.
pubkey() { grep -m1 -E '^pubkey' "$(cfg "$1")" | sed -E 's/.*"([0-9a-f]+)".*/\1/'; }

# The FROST group public key (the treasury's spend-validating key, ak).
group_key() { cat "$SPIKE/group.key"; }
