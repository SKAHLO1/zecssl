//! Unit tests: state transitions and accounting, without cross-contract calls.

use near_sdk::json_types::{U128, U64};
use near_sdk::test_utils::VMContextBuilder;
use near_sdk::{testing_env, AccountId, NearToken};

use crate::*;

const SEC: u64 = 1_000_000_000;
const USDC: u128 = 1_000_000;

fn acc(s: &str) -> AccountId {
    s.parse().unwrap()
}

fn config() -> Config {
    Config {
        token: acc("usdc.test"),
        token_symbol: "USDC".into(),
        token_decimals: 6,
        threshold: 2,
        arbiter_threshold: 2,
        min_bond: U128(1_000 * USDC),
        unbonding_delay_ns: U64(10_000 * SEC),
        release_window_ns: U64(600 * SEC),
        dispute_window_ns: U64(3_600 * SEC),
        challenge_window_ns: U64(3_600 * SEC),
        challenge_bond: U128(10 * USDC),
        per_order_cap: U128(5_000 * USDC),
        epoch_cap: U128(20_000 * USDC),
        epoch_length_ns: U64(86_400 * SEC),
    }
}

fn ctx(caller: &str, time: u64) {
    testing_env!(VMContextBuilder::new()
        .current_account_id(acc("ssl.test"))
        .predecessor_account_id(acc(caller))
        .block_timestamp(time)
        .attached_deposit(NearToken::from_yoctonear(0))
        .build());
}

/// Three bonded operators (2,000 / 3,000 / 4,000 USDC) and 10,000 USDC of liquidity.
fn setup() -> Contract {
    ctx("deployer.test", 1_000 * SEC);
    let mut c = Contract::new(config(), vec![acc("op1.test"), acc("op2.test"), acc("op3.test")], vec![acc("arb1.test"), acc("arb2.test"), acc("arb3.test")]);
    for (op, bond) in [("op1.test", 2_000), ("op2.test", 3_000), ("op3.test", 4_000)] {
        ctx("usdc.test", 1_000 * SEC);
        let _ = c.ft_on_transfer(acc(op), U128(bond * USDC), r#"{"action":"bond"}"#.into());
    }
    ctx("usdc.test", 1_000 * SEC);
    let _ = c.ft_on_transfer(acc("lp.test"), U128(10_000 * USDC), r#"{"action":"liquidity"}"#.into());
    c
}

fn terms(amount: u128) -> OrderTerms {
    OrderTerms { recipient: acc("alice.test"), amount_out: U128(amount * USDC), zec_amount_zat: U64(50 * 100_000_000), deposit_deadline: U64(2_000 * SEC) }
}

fn assert_balanced(c: &Contract, expected_total: u128) {
    assert_eq!(c.get_accounting().total.0, expected_total, "{:#?}", c.get_accounting());
}

#[test]
fn capacity_is_the_cheapest_colluding_quorum() {
    let c = setup();
    // The two smallest bonds: 2,000 + 3,000.
    assert_eq!(c.get_max_in_flight().0, 5_000 * USDC);
    assert_eq!(c.get_total_bonded().usd, 9_000.0);
    assert_eq!(c.get_liquidity()[0].amount, "10000");
    assert_balanced(&c, 19_000 * USDC);
}

#[test]
fn order_opens_at_threshold_and_reserves_liquidity() {
    let mut c = setup();
    ctx("op1.test", 1_100 * SEC);
    assert_eq!(c.approve_order("o1".into(), terms(1_000)), OrderState::Proposed);
    ctx("op2.test", 1_100 * SEC);
    assert_eq!(c.approve_order("o1".into(), terms(1_000)), OrderState::Open);
    let acct = c.get_accounting();
    assert_eq!(acct.reserved.0, 1_000 * USDC);
    assert_eq!(acct.available.0, 9_000 * USDC);
    assert_eq!(c.get_order("o1".into()).unwrap().release_deadline.0, 2_600 * SEC);
}

#[test]
fn equivocation_is_recorded_not_reverted() {
    let mut c = setup();
    ctx("op1.test", 1_100 * SEC);
    c.approve_order("o1".into(), terms(1_000));
    c.approve_order("o1".into(), terms(1_500));
    assert_eq!(c.get_operator(acc("op1.test")).unwrap().equivocations, 1);
    assert_eq!(c.get_order("o1".into()).unwrap().state, OrderState::Proposed);
}

#[test]
#[should_panic(expected = "terms differ")]
fn a_second_operator_cannot_change_terms() {
    let mut c = setup();
    ctx("op1.test", 1_100 * SEC);
    c.approve_order("o1".into(), terms(1_000));
    ctx("op2.test", 1_100 * SEC);
    c.approve_order("o1".into(), terms(1_500));
}

#[test]
#[should_panic(expected = "bonded capacity")]
fn orders_beyond_bonded_capacity_are_refused() {
    let mut c = setup();
    for (i, amount) in [(1, 4_000), (2, 1_500)] {
        for op in ["op1.test", "op2.test"] {
            ctx(op, 1_100 * SEC);
            c.approve_order(format!("o{i}"), terms(amount));
        }
    }
}

#[test]
#[should_panic(expected = "per-order cap")]
fn per_order_cap_applies() {
    let mut c = setup();
    for op in ["op1.test", "op2.test"] {
        ctx(op, 1_100 * SEC);
        c.approve_order("big".into(), terms(5_001));
    }
}

#[test]
fn release_needs_threshold_and_debits_liquidity() {
    let mut c = setup();
    for op in ["op1.test", "op2.test"] {
        ctx(op, 1_100 * SEC);
        c.approve_order("o1".into(), terms(1_000));
    }
    ctx("op1.test", 1_200 * SEC);
    assert!(c.approve_release("o1".into()).is_none());
    ctx("op3.test", 1_200 * SEC);
    assert!(c.approve_release("o1".into()).is_some());
    assert_eq!(c.get_order("o1".into()).unwrap().state, OrderState::Released);
    let acct = c.get_accounting();
    assert_eq!(acct.liquidity.0, 9_000 * USDC);
    assert_eq!(acct.reserved.0, 0);
    // 1,000 USDC is in transit to the recipient.
    assert_balanced(&c, 18_000 * USDC);
}

#[test]
fn upheld_dispute_pays_the_recipient_from_approver_bonds() {
    let mut c = setup();
    for op in ["op1.test", "op2.test"] {
        ctx(op, 1_100 * SEC);
        c.approve_order("o1".into(), terms(1_000));
    }
    // After the release deadline (2,600s), the recipient disputes with a 10 USDC bond.
    ctx("usdc.test", 2_700 * SEC);
    let unused = c.ft_on_transfer(acc("alice.test"), U128(15 * USDC), r#"{"action":"dispute","order_id":"o1"}"#.into());
    assert!(matches!(unused, near_sdk::PromiseOrValue::Value(U128(v)) if v == 5 * USDC));
    assert_eq!(c.get_order("o1".into()).unwrap().state, OrderState::Disputed);

    ctx("arb1.test", 2_800 * SEC);
    let id = c.propose(GovAction::ResolveDispute { order_id: "o1".into(), upheld: true });
    ctx("arb2.test", 2_800 * SEC);
    c.approve_proposal(id);

    assert_eq!(c.get_order("o1".into()).unwrap().state, OrderState::Compensated);
    // 500 USDC slashed from each approver; liquidity untouched and unreserved.
    assert_eq!(c.get_operator(acc("op1.test")).unwrap().bond.0, 1_500 * USDC);
    assert_eq!(c.get_operator(acc("op2.test")).unwrap().bond.0, 2_500 * USDC);
    assert_eq!(c.get_operator(acc("op3.test")).unwrap().bond.0, 4_000 * USDC);
    let acct = c.get_accounting();
    assert_eq!(acct.liquidity.0, 10_000 * USDC);
    assert_eq!(acct.reserved.0, 0);
    assert_eq!(acct.challenge_bonds.0, 0);
}

#[test]
#[should_panic(expected = "only the recipient")]
fn only_the_recipient_can_dispute() {
    let mut c = setup();
    for op in ["op1.test", "op2.test"] {
        ctx(op, 1_100 * SEC);
        c.approve_order("o1".into(), terms(1_000));
    }
    ctx("usdc.test", 2_700 * SEC);
    let _ = c.ft_on_transfer(acc("mallory.test"), U128(10 * USDC), r#"{"action":"dispute","order_id":"o1"}"#.into());
}

#[test]
fn open_orders_expire_after_the_dispute_window() {
    let mut c = setup();
    for op in ["op1.test", "op2.test"] {
        ctx(op, 1_100 * SEC);
        c.approve_order("o1".into(), terms(1_000));
    }
    ctx("anyone.test", 2_600 * SEC + 3_601 * SEC);
    assert_eq!(c.close_expired("o1".into()), OrderState::Expired);
    assert_eq!(c.get_accounting().reserved.0, 0);
}

#[test]
fn direction_two_settles_into_liquidity_without_a_challenge() {
    let mut c = setup();
    ctx("usdc.test", 1_100 * SEC);
    let addr = format!("utest1{}", "q".repeat(70));
    let _ = c.ft_on_transfer(
        acc("bob.test"),
        U128(500 * USDC),
        format!(r#"{{"action":"lock_for_zec","order_id":"d1","zec_address":"{addr}","zec_amount_zat":"40000000","deadline":"2000000000000"}}"#),
    );
    assert_eq!(c.get_accounting().escrowed.0, 500 * USDC);
    let txid = "ab".repeat(32);
    for op in ["op2.test", "op3.test"] {
        ctx(op, 1_200 * SEC);
        c.claim_settlement("d1".into(), txid.clone());
    }
    assert_eq!(c.get_order("d1".into()).unwrap().state, OrderState::Claimed);
    ctx("anyone.test", 1_200 * SEC + 3_601 * SEC);
    assert_eq!(c.finalize_settlement("d1".into()), OrderState::Settled);
    let acct = c.get_accounting();
    assert_eq!(acct.escrowed.0, 0);
    assert_eq!(acct.liquidity.0, 10_500 * USDC);
    assert_balanced(&c, 19_500 * USDC);
}

#[test]
fn upheld_challenge_refunds_the_user_and_slashes_claimers() {
    let mut c = setup();
    ctx("usdc.test", 1_100 * SEC);
    let addr = format!("utest1{}", "q".repeat(70));
    let _ = c.ft_on_transfer(
        acc("bob.test"),
        U128(600 * USDC),
        format!(r#"{{"action":"lock_for_zec","order_id":"d1","zec_address":"{addr}","zec_amount_zat":"40000000","deadline":"2000000000000"}}"#),
    );
    for op in ["op2.test", "op3.test"] {
        ctx(op, 1_200 * SEC);
        c.claim_settlement("d1".into(), "cd".repeat(32));
    }
    ctx("usdc.test", 1_300 * SEC);
    let _ = c.ft_on_transfer(acc("bob.test"), U128(10 * USDC), r#"{"action":"challenge","order_id":"d1"}"#.into());
    for arb in ["arb1.test", "arb3.test"] {
        ctx(arb, 1_400 * SEC);
        if arb == "arb1.test" {
            c.propose(GovAction::ResolveDispute { order_id: "d1".into(), upheld: true });
        } else {
            c.approve_proposal(0);
        }
    }
    assert_eq!(c.get_order("d1".into()).unwrap().state, OrderState::Reimbursed);
    assert_eq!(c.get_operator(acc("op2.test")).unwrap().bond.0, 2_700 * USDC);
    assert_eq!(c.get_operator(acc("op3.test")).unwrap().bond.0, 3_700 * USDC);
    assert_eq!(c.get_accounting().insurance.0, 600 * USDC);
}

#[test]
#[should_panic(expected = "shielded unified address")]
fn direction_two_refuses_transparent_payout_addresses() {
    let mut c = setup();
    ctx("usdc.test", 1_100 * SEC);
    let _ = c.ft_on_transfer(
        acc("bob.test"),
        U128(100 * USDC),
        r#"{"action":"lock_for_zec","order_id":"d1","zec_address":"tmBsTi2xWTjUdEXnuTceL7fecEQKeWaPDJd","zec_amount_zat":"1000","deadline":"2000000000000"}"#.into(),
    );
}

#[test]
fn unbonded_funds_stay_slashable_until_unlocked() {
    let mut c = setup();
    ctx("op3.test", 1_100 * SEC);
    c.request_unbond(U128(1_000 * USDC));
    let op = c.get_operator(acc("op3.test")).unwrap();
    assert_eq!((op.bond.0, op.unbonding.0), (3_000 * USDC, 1_000 * USDC));
    for arb in ["arb1.test", "arb2.test"] {
        ctx(arb, 1_200 * SEC);
        if arb == "arb1.test" {
            c.propose(GovAction::Slash { operator: acc("op3.test"), amount: U128(3_500 * USDC), reason: "test".into() });
        } else {
            c.approve_proposal(0);
        }
    }
    let op = c.get_operator(acc("op3.test")).unwrap();
    assert_eq!((op.bond.0, op.unbonding.0), (0, 500 * USDC));
    assert_eq!(c.get_accounting().insurance.0, 3_500 * USDC);
}

#[test]
#[should_panic(expected = "unbonding delay has not passed")]
fn cannot_withdraw_before_the_unbonding_delay() {
    let mut c = setup();
    ctx("op3.test", 1_100 * SEC);
    c.request_unbond(U128(1_000 * USDC));
    ctx("op3.test", 1_100 * SEC + 9_999 * SEC);
    let _ = c.withdraw_unbonded();
}

#[test]
#[should_panic(expected = "paused")]
fn pause_blocks_new_orders() {
    let mut c = setup();
    for arb in ["arb1.test", "arb2.test"] {
        ctx(arb, 1_100 * SEC);
        if arb == "arb1.test" {
            c.propose(GovAction::Pause);
        } else {
            c.approve_proposal(0);
        }
    }
    ctx("op1.test", 1_100 * SEC);
    c.approve_order("o1".into(), terms(100));
}

#[test]
#[should_panic(expected = "not an arbiter")]
fn operators_cannot_govern() {
    let mut c = setup();
    ctx("op1.test", 1_100 * SEC);
    c.propose(GovAction::Unpause);
}

#[test]
#[should_panic(expected = "only the vault token")]
fn other_tokens_are_refused() {
    let mut c = setup();
    ctx("fake-usdc.test", 1_100 * SEC);
    let _ = c.ft_on_transfer(acc("op1.test"), U128(1), r#"{"action":"liquidity"}"#.into());
}

#[test]
fn format_units_trims_trailing_zeros() {
    assert_eq!(format_units(1_234_500_000, 6), "1234.5");
    assert_eq!(format_units(0, 6), "0");
    assert_eq!(format_units(1, 6), "0.000001");
}
