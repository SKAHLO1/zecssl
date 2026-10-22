//! What the coordinator proposes and what each operator independently checks before
//! approving it on NEAR. Pure functions only: every input (time, prices, chain data) is a
//! parameter, so every rule is unit-tested and operators can't be talked past them.
//!
//! The coordinator is untrusted. An operator approves an order only if the deposit address
//! is the treasury's own derivation for the stated index, the payout does not exceed what
//! the operator's own price feed allows, and the terms are within bounds. It approves a
//! release only once its own wallet shows the deposit paid in full and confirmed.

use serde::{Deserialize, Serialize};

/// The terms written to the NEAR vault (`approve_order`). Amounts are integers in base
/// units; the deadline is nanoseconds since the Unix epoch, like NEAR block timestamps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderTerms {
    pub recipient: String,
    /// Vault token base units (USDC: 6 decimals), as a decimal string on the wire.
    #[serde(with = "u128_string")]
    pub amount_out: u128,
    #[serde(with = "u64_string")]
    pub zec_amount_zat: u64,
    #[serde(with = "u64_string")]
    pub deposit_deadline: u64,
}

/// What the coordinator sends each operator.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    pub order_id: String,
    pub terms: OrderTerms,
    /// Diversifier index of the deposit address within the treasury viewing key. Random,
    /// so addresses never repeat even if the coordinator's state is reset.
    pub diversifier_index: u64,
    pub deposit_address: String,
    /// The ZEC/USD price the coordinator quoted from, for the record.
    pub quoted_zec_usd: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Limits {
    /// The operator spread the quote must include, in basis points.
    pub spread_bps: u32,
    /// How far above the operator's own price (after spread) a quote may pay, in bps.
    pub price_tolerance_bps: u32,
    pub token_decimals: u8,
    pub max_amount_out: u128,
    pub min_zec_zat: u64,
    /// Deposit deadline must be at least this far out when proposed (ns).
    pub min_deposit_window_ns: u64,
    /// ...and at most this far out (ns).
    pub max_deposit_window_ns: u64,
    /// Indices below this are reserved (index 0 is the treasury's default address).
    pub min_diversifier_index: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rejection {
    BadOrderId,
    AddressMismatch { expected: String },
    ReservedIndex,
    BadRecipient,
    TooSmall,
    TooLarge,
    DeadlineTooSoon,
    DeadlineTooFar,
    /// The quote pays out more than the operator's own price allows.
    Overpays { max_amount_out: u128 },
    NoPrice,
    /// The order id is already on the vault with different terms. Approving would be
    /// equivocation, which the vault records and arbiters can slash.
    IdConflict,
}

impl std::fmt::Display for Rejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Rejection::BadOrderId => f.write_str("order id must be 1-64 characters of [A-Za-z0-9_-]"),
            Rejection::AddressMismatch { expected } => write!(f, "deposit address is not the treasury's address for that index (expected {expected})"),
            Rejection::ReservedIndex => f.write_str("diversifier index is reserved"),
            Rejection::BadRecipient => f.write_str("recipient is not a valid NEAR account id"),
            Rejection::TooSmall => f.write_str("order is below the minimum"),
            Rejection::TooLarge => f.write_str("order is above the maximum"),
            Rejection::DeadlineTooSoon => f.write_str("deposit deadline is too soon"),
            Rejection::DeadlineTooFar => f.write_str("deposit deadline is too far out"),
            Rejection::Overpays { max_amount_out } => write!(f, "quote pays out more than this operator's price allows (max {max_amount_out})"),
            Rejection::NoPrice => f.write_str("operator has no current ZEC price"),
            Rejection::IdConflict => f.write_str("this order id already exists on the vault with different terms"),
        }
    }
}

pub fn valid_order_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// NEAR account ids: 2-64 chars; named accounts are dot-separated parts of lowercase
/// alphanumerics joined by single `-` or `_`; implicit accounts are 64 hex chars.
pub fn valid_near_account(id: &str) -> bool {
    if id.len() < 2 || id.len() > 64 {
        return false;
    }
    if id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
        return true;
    }
    id.split('.').all(|part| {
        !part.is_empty()
            && part.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
            && !part.starts_with(['-', '_'])
            && !part.ends_with(['-', '_'])
            && !part.contains("--")
            && !part.contains("__")
            && !part.contains("-_")
            && !part.contains("_-")
    })
}

/// The most a quote may pay for `zec_amount_zat` given the operator's own ZEC/USD price.
pub fn max_amount_out(zec_amount_zat: u64, own_zec_usd: f64, limits: &Limits) -> u128 {
    let zec = zec_amount_zat as f64 / 1e8;
    let after_spread = own_zec_usd * (1.0 - limits.spread_bps as f64 / 10_000.0);
    let ceiling = after_spread * (1.0 + limits.price_tolerance_bps as f64 / 10_000.0);
    (zec * ceiling * 10f64.powi(limits.token_decimals as i32)).floor() as u128
}

/// The checks an operator runs before calling `approve_order`. `derived_address` is the
/// operator's own derivation for `proposal.diversifier_index`.
pub fn check_proposal(p: &Proposal, derived_address: &str, own_zec_usd: Option<f64>, now_ns: u64, limits: &Limits) -> Result<(), Rejection> {
    if !valid_order_id(&p.order_id) {
        return Err(Rejection::BadOrderId);
    }
    if p.diversifier_index < limits.min_diversifier_index {
        return Err(Rejection::ReservedIndex);
    }
    if p.deposit_address != derived_address {
        return Err(Rejection::AddressMismatch { expected: derived_address.to_string() });
    }
    if !valid_near_account(&p.terms.recipient) {
        return Err(Rejection::BadRecipient);
    }
    if p.terms.zec_amount_zat < limits.min_zec_zat || p.terms.amount_out == 0 {
        return Err(Rejection::TooSmall);
    }
    if p.terms.amount_out > limits.max_amount_out {
        return Err(Rejection::TooLarge);
    }
    let window = p.terms.deposit_deadline.saturating_sub(now_ns);
    if p.terms.deposit_deadline <= now_ns || window < limits.min_deposit_window_ns {
        return Err(Rejection::DeadlineTooSoon);
    }
    if window > limits.max_deposit_window_ns {
        return Err(Rejection::DeadlineTooFar);
    }
    let price = own_zec_usd.filter(|p| p.is_finite() && *p > 0.0).ok_or(Rejection::NoPrice)?;
    let max = max_amount_out(p.terms.zec_amount_zat, price, limits);
    if p.terms.amount_out > max {
        return Err(Rejection::Overpays { max_amount_out: max });
    }
    Ok(())
}

/// Checks a proposal against what the vault already holds for that order id. An operator
/// must never approve different terms for an id that exists: the vault would record it as
/// equivocation. Identical terms are fine (approving again is a no-op).
pub fn check_against_vault(p: &Proposal, on_vault: Option<&OrderTerms>) -> Result<(), Rejection> {
    match on_vault {
        Some(terms) if *terms != p.terms => Err(Rejection::IdConflict),
        _ => Ok(()),
    }
}

/// A deposit as an operator's wallet reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeenDeposit {
    pub txid: String,
    pub mined_height: Option<u32>,
    pub value_zat: u64,
    /// When this operator first saw it mined (ns).
    pub first_seen_mined_ns: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DepositStatus {
    /// Nothing has arrived.
    None,
    /// Something arrived but is not yet deep enough to rely on.
    Confirming { received_zat: u64, confirmations: u32 },
    /// Confirmed, but less than the order requires: refund path.
    Underpaid { received_zat: u64 },
    /// Confirmed in full, but first mined after the deadline: refund path.
    Late { received_zat: u64 },
    /// Confirmed in full, on time: release.
    Paid { received_zat: u64, txids: Vec<String> },
}

/// Classifies what has arrived at an order's address. Only mined deposits with at least
/// `confirmations` blocks count; unmined or shallow ones only report progress.
pub fn deposit_status(deposits: &[SeenDeposit], expected_zat: u64, tip_height: u32, confirmations: u32, deadline_ns: u64) -> DepositStatus {
    let depth = |d: &SeenDeposit| d.mined_height.map_or(0, |h| tip_height.saturating_sub(h) + 1);
    let confirmed: Vec<&SeenDeposit> = deposits.iter().filter(|d| depth(d) >= confirmations).collect();
    let confirmed_zat: u64 = confirmed.iter().map(|d| d.value_zat).sum();
    if confirmed_zat >= expected_zat && expected_zat > 0 {
        let late = confirmed.iter().any(|d| d.first_seen_mined_ns.is_some_and(|t| t > deadline_ns));
        if late {
            return DepositStatus::Late { received_zat: confirmed_zat };
        }
        return DepositStatus::Paid { received_zat: confirmed_zat, txids: confirmed.iter().map(|d| d.txid.clone()).collect() };
    }
    let any_zat: u64 = deposits.iter().map(|d| d.value_zat).sum();
    if any_zat == 0 {
        return DepositStatus::None;
    }
    let pending = deposits.iter().any(|d| depth(d) < confirmations);
    if !pending {
        return DepositStatus::Underpaid { received_zat: confirmed_zat };
    }
    let min_depth = deposits.iter().map(depth).min().unwrap_or(0);
    DepositStatus::Confirming { received_zat: any_zat, confirmations: min_depth }
}

mod u128_string {
    use serde::{de::Error, Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &u128, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u128, D::Error> {
        String::deserialize(d)?.parse().map_err(D::Error::custom)
    }
}

mod u64_string {
    use serde::{de::Error, Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        String::deserialize(d)?.parse().map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEC: u64 = 1_000_000_000;
    const NOW: u64 = 1_800_000_000 * SEC;

    fn limits() -> Limits {
        Limits {
            spread_bps: 25,
            price_tolerance_bps: 100,
            token_decimals: 6,
            max_amount_out: 1_000_000_000,
            min_zec_zat: 100_000,
            min_deposit_window_ns: 10 * 60 * SEC,
            max_deposit_window_ns: 2 * 3600 * SEC,
            min_diversifier_index: 1_000,
        }
    }

    /// 0.5 ZEC at $1,000 minus 0.25% = 498.75 USDC.
    fn proposal() -> Proposal {
        Proposal {
            order_id: "zs-1001".into(),
            terms: OrderTerms { recipient: "alice.testnet".into(), amount_out: 498_750_000, zec_amount_zat: 50_000_000, deposit_deadline: NOW + 30 * 60 * SEC },
            diversifier_index: 1_001,
            deposit_address: "utest1abc".into(),
            quoted_zec_usd: 1_000.0,
        }
    }

    #[test]
    fn a_fair_proposal_passes() {
        assert_eq!(check_proposal(&proposal(), "utest1abc", Some(1_000.0), NOW, &limits()), Ok(()));
    }

    #[test]
    fn a_coordinator_supplied_address_must_match_the_operators_derivation() {
        let err = check_proposal(&proposal(), "utest1other", Some(1_000.0), NOW, &limits()).unwrap_err();
        assert!(matches!(err, Rejection::AddressMismatch { .. }));
    }

    #[test]
    fn overpaying_quotes_are_refused_but_price_noise_is_tolerated() {
        // Operator sees $990: max = 0.5 * 990 * 0.9975 * 1.01 = 498.74 USDC; quote 498.75 is over.
        assert!(matches!(check_proposal(&proposal(), "utest1abc", Some(990.0), NOW, &limits()), Err(Rejection::Overpays { .. })));
        // Operator sees $995: max = 501.25 USDC; within tolerance.
        assert_eq!(check_proposal(&proposal(), "utest1abc", Some(995.0), NOW, &limits()), Ok(()));
    }

    #[test]
    fn missing_prices_block_approval() {
        assert_eq!(check_proposal(&proposal(), "utest1abc", None, NOW, &limits()), Err(Rejection::NoPrice));
        assert_eq!(check_proposal(&proposal(), "utest1abc", Some(f64::NAN), NOW, &limits()), Err(Rejection::NoPrice));
    }

    #[test]
    fn deadlines_must_be_within_bounds() {
        let mut p = proposal();
        p.terms.deposit_deadline = NOW + 5 * 60 * SEC;
        assert_eq!(check_proposal(&p, "utest1abc", Some(1_000.0), NOW, &limits()), Err(Rejection::DeadlineTooSoon));
        p.terms.deposit_deadline = NOW + 3 * 3600 * SEC;
        assert_eq!(check_proposal(&p, "utest1abc", Some(1_000.0), NOW, &limits()), Err(Rejection::DeadlineTooFar));
        p.terms.deposit_deadline = NOW - SEC;
        assert_eq!(check_proposal(&p, "utest1abc", Some(1_000.0), NOW, &limits()), Err(Rejection::DeadlineTooSoon));
    }

    #[test]
    fn reserved_indices_and_bad_recipients_are_refused() {
        let mut p = proposal();
        p.diversifier_index = 0;
        assert_eq!(check_proposal(&p, "utest1abc", Some(1_000.0), NOW, &limits()), Err(Rejection::ReservedIndex));
        let mut p = proposal();
        p.terms.recipient = "Alice.testnet".into();
        assert_eq!(check_proposal(&p, "utest1abc", Some(1_000.0), NOW, &limits()), Err(Rejection::BadRecipient));
    }

    #[test]
    fn an_existing_id_with_other_terms_is_refused() {
        let p = proposal();
        assert_eq!(check_against_vault(&p, None), Ok(()));
        assert_eq!(check_against_vault(&p, Some(&p.terms)), Ok(()));
        let mut other = p.terms.clone();
        other.amount_out += 1;
        assert_eq!(check_against_vault(&p, Some(&other)), Err(Rejection::IdConflict));
    }

    #[test]
    fn terms_round_trip_the_contracts_json() {
        let json = r#"{"recipient":"alice.testnet","amount_out":"498750000","zec_amount_zat":"50000000","deposit_deadline":"1801800000000000000"}"#;
        let terms: OrderTerms = serde_json::from_str(json).unwrap();
        assert_eq!(terms.amount_out, 498_750_000);
        assert_eq!(serde_json::to_string(&terms).unwrap(), json);
    }

    #[test]
    fn near_account_validation() {
        for ok in ["alice.testnet", "zecssl-skahlo-op1.testnet", "a_b.near", &"a".repeat(64)] {
            assert!(valid_near_account(ok), "{ok}");
        }
        for bad in ["A.testnet", "a..b", "-a.near", "a-.near", "a", "a--b.near", &"x".repeat(65)] {
            assert!(!valid_near_account(bad), "{bad}");
        }
    }

    fn dep(h: Option<u32>, v: u64, seen: Option<u64>) -> SeenDeposit {
        SeenDeposit { txid: format!("t{v}"), mined_height: h, value_zat: v, first_seen_mined_ns: seen }
    }

    #[test]
    fn deposit_status_progresses_from_none_to_paid() {
        let deadline = NOW;
        assert_eq!(deposit_status(&[], 100, 1_000, 12, deadline), DepositStatus::None);
        assert_eq!(
            deposit_status(&[dep(None, 100, None)], 100, 1_000, 12, deadline),
            DepositStatus::Confirming { received_zat: 100, confirmations: 0 }
        );
        assert_eq!(
            deposit_status(&[dep(Some(995), 100, Some(NOW - SEC))], 100, 1_000, 12, deadline),
            DepositStatus::Confirming { received_zat: 100, confirmations: 6 }
        );
        assert_eq!(
            deposit_status(&[dep(Some(989), 100, Some(NOW - SEC))], 100, 1_000, 12, deadline),
            DepositStatus::Paid { received_zat: 100, txids: vec!["t100".into()] }
        );
    }

    #[test]
    fn several_deposits_add_up() {
        let d = [dep(Some(980), 60, Some(NOW - SEC)), dep(Some(985), 40, Some(NOW - SEC))];
        assert!(matches!(deposit_status(&d, 100, 1_000, 12, NOW), DepositStatus::Paid { received_zat: 100, .. }));
    }

    #[test]
    fn short_or_late_deposits_go_to_refund() {
        assert_eq!(deposit_status(&[dep(Some(980), 90, Some(NOW - SEC))], 100, 1_000, 12, NOW), DepositStatus::Underpaid { received_zat: 90 });
        assert_eq!(deposit_status(&[dep(Some(980), 100, Some(NOW + SEC))], 100, 1_000, 12, NOW), DepositStatus::Late { received_zat: 100 });
    }

    #[test]
    fn terms_serialize_like_the_contract_expects() {
        let json = serde_json::to_value(&proposal().terms).unwrap();
        assert_eq!(json["amount_out"], "498750000");
        assert_eq!(json["zec_amount_zat"], "50000000");
        assert!(json["deposit_deadline"].is_string());
    }
}
