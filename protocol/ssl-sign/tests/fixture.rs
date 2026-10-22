//! Regression tests against a real PCZT: the one that became Zcash testnet transaction
//! a5533fe75575b09d8986a05005d2c0528cf45a1a7e4cc71b304ae76a9e14487d, a v6 transaction
//! whose spend was authorized by a 2-of-3 rerandomized FROST signature. The fixture is
//! copied from ZcashFoundation/frost-tools (zcash-sign/tests/fixtures).

const FIXTURE: &[u8] = include_bytes!("fixtures/ironwood_v6.pczt");

/// The v6 sighash the on-chain transaction was signed against. For a fully shielded
/// transaction, ZIP 244 makes the txid the byte reversal of this value.
const SIGHASH: &str = "7d48149e6ae74a301bc74c7e1a5af48c52c0d20550a086899db07555e73f53a5";
const TXID: &str = "a5533fe75575b09d8986a05005d2c0528cf45a1a7e4cc71b304ae76a9e14487d";

#[test]
fn sighash_matches_the_on_chain_transaction() {
    let request = ssl_sign::prepare(FIXTURE).expect("fixture prepares");
    assert_eq!(request.effects.tx_version, "V6");
    assert_eq!(request.sighash, SIGHASH);

    let mut reversed = hex::decode(SIGHASH).unwrap();
    reversed.reverse();
    assert_eq!(hex::encode(reversed), TXID);
}

#[test]
fn finds_the_spend_awaiting_signature() {
    let request = ssl_sign::prepare(FIXTURE).expect("fixture prepares");
    assert_eq!(request.spends.len(), 1, "{request:#?}");
    assert_eq!(request.spends[0].alpha.len(), 64);
}

#[test]
fn reports_a_fully_shielded_transaction() {
    let request = ssl_sign::prepare(FIXTURE).expect("fixture prepares");
    assert!(request.effects.is_fully_shielded(), "{:#?}", request.effects);
}

#[test]
fn rejects_a_signature_that_does_not_verify() {
    let request = ssl_sign::prepare(FIXTURE).expect("fixture prepares");
    let bogus = ssl_sign::SpendSignature {
        pool: request.spends[0].pool,
        index: request.spends[0].index,
        signature: "11".repeat(64),
    };
    assert!(ssl_sign::apply(FIXTURE, &[bogus]).is_err());
}

#[test]
fn refuses_to_apply_with_a_spend_left_unsigned() {
    assert!(ssl_sign::apply(FIXTURE, &[]).is_err());
}
