//! Integration tests against a local NEAR sandbox node, using the real wasm of the ZEC SSL
//! contract and the test token. Build the wasm first (scripts/test.sh does).
//!
//! The invariant checked throughout: the token balance the contract actually holds
//! equals the total of its own accounting (liquidity + escrow + bonds + insurance +
//! challenge bonds + claimable). This is what a reserve attestation verifies.

use near_workspaces::network::Sandbox;
use near_workspaces::types::NearToken;
use near_workspaces::{Account, Contract, Worker};
use serde_json::{json, Value};

const USDC: u128 = 1_000_000;
const SEC: u64 = 1_000_000_000;

struct Env {
    worker: Worker<Sandbox>,
    token: Contract,
    ssl: Contract,
    ops: Vec<Account>,
    arbs: Vec<Account>,
}

async fn now(worker: &Worker<Sandbox>) -> anyhow::Result<u64> {
    Ok(worker.view_block().await?.timestamp())
}

/// Produces blocks until the chain's clock passes `t` (ns). Sandbox block time is not a
/// fixed duration per block, so tests wait on the timestamp itself.
async fn advance_past(worker: &Worker<Sandbox>, t: u64) -> anyhow::Result<()> {
    for _ in 0..200 {
        if now(worker).await? > t {
            return Ok(());
        }
        worker.fast_forward(10).await?;
    }
    anyhow::bail!("chain time did not pass {t}")
}

async fn register(token: &Contract, who: &Account) -> anyhow::Result<()> {
    who.call(token.id(), "storage_deposit")
        .args_json(json!({ "account_id": who.id() }))
        .deposit(NearToken::from_millinear(10))
        .transact()
        .await?
        .into_result()?;
    Ok(())
}

async fn faucet(token: &Contract, who: &Account, amount: u128) -> anyhow::Result<()> {
    who.call(token.id(), "faucet")
        .args_json(json!({ "amount": amount.to_string() }))
        .deposit(NearToken::from_millinear(10))
        .transact()
        .await?
        .into_result()?;
    Ok(())
}

async fn send(token: &Contract, from: &Account, to: &Contract, amount: u128, msg: Value) -> anyhow::Result<()> {
    from.call(token.id(), "ft_transfer_call")
        .args_json(json!({ "receiver_id": to.id(), "amount": amount.to_string(), "msg": msg.to_string() }))
        .deposit(NearToken::from_yoctonear(1))
        .max_gas()
        .transact()
        .await?
        .into_result()?;
    Ok(())
}

async fn balance(token: &Contract, id: &near_workspaces::AccountId) -> anyhow::Result<u128> {
    let v: String = token.view("ft_balance_of").args_json(json!({ "account_id": id })).await?.json()?;
    Ok(v.parse()?)
}

async fn accounting(ssl: &Contract) -> anyhow::Result<Value> {
    Ok(ssl.view("get_accounting").await?.json()?)
}

fn u(v: &Value) -> u128 {
    v.as_str().expect("U128 is a string").parse().unwrap()
}

async fn assert_reserves(e: &Env) -> anyhow::Result<()> {
    let acct = accounting(&e.ssl).await?;
    let held = balance(&e.token, e.ssl.id()).await?;
    assert_eq!(held, u(&acct["total"]), "token balance != accounted total: {acct:#}");
    Ok(())
}

async fn setup() -> anyhow::Result<Env> {
    let worker = near_workspaces::sandbox().await?;
    let token = worker.dev_deploy(&std::fs::read("../res/zec_ssl_test_token.wasm")?).await?;
    token.call("new").args_json(json!({ "symbol": "USDC", "name": "ZEC SSL test USDC" })).transact().await?.into_result()?;
    let ssl = worker.dev_deploy(&std::fs::read("../res/zec_ssl_contract.wasm")?).await?;

    let mut ops = vec![];
    for _ in 0..3 {
        ops.push(worker.dev_create_account().await?);
    }
    let mut arbs = vec![];
    for _ in 0..3 {
        arbs.push(worker.dev_create_account().await?);
    }

    ssl.call("new")
        .args_json(json!({
            "config": {
                "token": token.id(),
                "token_symbol": "USDC",
                "token_decimals": 6,
                "threshold": 2,
                "arbiter_threshold": 2,
                "min_bond": (1_000 * USDC).to_string(),
                "unbonding_delay_ns": (100 * SEC).to_string(),
                "release_window_ns": (5 * SEC).to_string(),
                "dispute_window_ns": (30 * SEC).to_string(),
                "challenge_window_ns": (5 * SEC).to_string(),
                "challenge_bond": (10 * USDC).to_string(),
                "per_order_cap": (5_000 * USDC).to_string(),
                "epoch_cap": (20_000 * USDC).to_string(),
                "epoch_length_ns": (86_400 * SEC).to_string()
            },
            "operators": ops.iter().map(|a| a.id()).collect::<Vec<_>>(),
            "arbiters": arbs.iter().map(|a| a.id()).collect::<Vec<_>>(),
        }))
        .transact()
        .await?
        .into_result()?;

    // The contract must be registered with the token to receive it.
    token.as_account().call(token.id(), "storage_deposit")
        .args_json(json!({ "account_id": ssl.id() }))
        .deposit(NearToken::from_millinear(10))
        .transact()
        .await?
        .into_result()?;

    for (op, bond) in ops.iter().zip([2_000u128, 3_000, 4_000]) {
        faucet(&token, op, 10_000 * USDC).await?;
        send(&token, op, &ssl, bond * USDC, json!({ "action": "bond" })).await?;
    }
    send(&token, &ops[0], &ssl, 5_000 * USDC, json!({ "action": "liquidity" })).await?;

    Ok(Env { worker, token, ssl, ops, arbs })
}

async fn approve_order(e: &Env, op: &Account, id: &str, recipient: &Account, amount: u128, deadline: u64) -> anyhow::Result<Value> {
    let r = op
        .call(e.ssl.id(), "approve_order")
        .args_json(json!({
            "order_id": id,
            "terms": {
                "recipient": recipient.id(),
                "amount_out": amount.to_string(),
                "zec_amount_zat": "5000000000",
                "deposit_deadline": deadline.to_string()
            }
        }))
        .max_gas()
        .transact()
        .await?
        .into_result()?;
    Ok(r.json()?)
}

#[tokio::test]
async fn direction_one_release_pays_the_recipient() -> anyhow::Result<()> {
    let e = setup().await?;
    assert_reserves(&e).await?;
    let alice = e.worker.dev_create_account().await?;
    register(&e.token, &alice).await?;
    let deadline = now(&e.worker).await? + 60 * SEC;

    assert_eq!(approve_order(&e, &e.ops[0], "o1", &alice, 1_000 * USDC, deadline).await?, json!("proposed"));
    assert_eq!(approve_order(&e, &e.ops[1], "o1", &alice, 1_000 * USDC, deadline).await?, json!("open"));
    assert_eq!(u(&accounting(&e.ssl).await?["reserved"]), 1_000 * USDC);

    for op in [&e.ops[1], &e.ops[2]] {
        op.call(e.ssl.id(), "approve_release").args_json(json!({ "order_id": "o1" })).max_gas().transact().await?.into_result()?;
    }
    assert_eq!(balance(&e.token, alice.id()).await?, 1_000 * USDC);
    let order: Value = e.ssl.view("get_order").args_json(json!({ "order_id": "o1" })).await?.json()?;
    assert_eq!(order["state"], "released");
    assert_reserves(&e).await?;

    // The website's views.
    let liq: Value = e.ssl.view("get_liquidity").await?.json()?;
    assert_eq!(liq[0]["amount"], "4000");
    let bonded: Value = e.ssl.view("get_total_bonded").await?.json()?;
    assert_eq!(bonded["usd"], 9000.0);
    Ok(())
}

#[tokio::test]
async fn failed_payouts_become_claimable() -> anyhow::Result<()> {
    let e = setup().await?;
    // Recipient never registered with the token, so the release transfer fails.
    let bob = e.worker.dev_create_account().await?;
    let deadline = now(&e.worker).await? + 60 * SEC;
    for op in [&e.ops[0], &e.ops[1]] {
        approve_order(&e, op, "o2", &bob, 700 * USDC, deadline).await?;
    }
    for op in [&e.ops[0], &e.ops[1]] {
        op.call(e.ssl.id(), "approve_release").args_json(json!({ "order_id": "o2" })).max_gas().transact().await?.into_result()?;
    }
    let claimable: String = e.ssl.view("get_claimable").args_json(json!({ "account_id": bob.id() })).await?.json()?;
    assert_eq!(claimable, (700 * USDC).to_string());
    assert_reserves(&e).await?;

    register(&e.token, &bob).await?;
    bob.call(e.ssl.id(), "claim").max_gas().transact().await?.into_result()?;
    assert_eq!(balance(&e.token, bob.id()).await?, 700 * USDC);
    assert_reserves(&e).await?;
    Ok(())
}

#[tokio::test]
async fn equivocation_is_recorded_on_chain() -> anyhow::Result<()> {
    let e = setup().await?;
    let alice = e.worker.dev_create_account().await?;
    let deadline = now(&e.worker).await? + 60 * SEC;
    approve_order(&e, &e.ops[0], "o3", &alice, 100 * USDC, deadline).await?;
    approve_order(&e, &e.ops[0], "o3", &alice, 999 * USDC, deadline).await?;
    let op: Value = e.ssl.view("get_operator").args_json(json!({ "account_id": e.ops[0].id() })).await?.json()?;
    assert_eq!(op["equivocations"], 1);
    Ok(())
}

#[tokio::test]
async fn direction_two_lock_claim_and_settle() -> anyhow::Result<()> {
    let e = setup().await?;
    let carol = e.worker.dev_create_account().await?;
    faucet(&e.token, &carol, 1_000 * USDC).await?;
    let deadline = now(&e.worker).await? + 60 * SEC;
    let addr = format!("utest1{}", "q".repeat(70));
    send(
        &e.token,
        &carol,
        &e.ssl,
        800 * USDC,
        json!({ "action": "lock_for_zec", "order_id": "d1", "zec_address": addr, "zec_amount_zat": "60000000", "deadline": deadline.to_string() }),
    )
    .await?;
    assert_eq!(u(&accounting(&e.ssl).await?["escrowed"]), 800 * USDC);
    assert_reserves(&e).await?;

    let txid = "ef".repeat(32);
    for op in [&e.ops[0], &e.ops[2]] {
        op.call(e.ssl.id(), "claim_settlement").args_json(json!({ "order_id": "d1", "zcash_txid": txid })).max_gas().transact().await?.into_result()?;
    }
    // Too early: the challenge window is still open.
    let early = e.ops[1].call(e.ssl.id(), "finalize_settlement").args_json(json!({ "order_id": "d1" })).transact().await?;
    assert!(early.is_failure());

    let order: Value = e.ssl.view("get_order").args_json(json!({ "order_id": "d1" })).await?.json()?;
    advance_past(&e.worker, u(&order["claimed_at"]) as u64 + 5 * SEC).await?;
    e.ops[1].call(e.ssl.id(), "finalize_settlement").args_json(json!({ "order_id": "d1" })).transact().await?.into_result()?;
    let acct = accounting(&e.ssl).await?;
    assert_eq!(u(&acct["escrowed"]), 0);
    assert_eq!(u(&acct["liquidity"]), 5_800 * USDC);
    assert_reserves(&e).await?;
    Ok(())
}

#[tokio::test]
async fn upheld_dispute_compensates_from_bonds() -> anyhow::Result<()> {
    let e = setup().await?;
    let dave = e.worker.dev_create_account().await?;
    faucet(&e.token, &dave, 100 * USDC).await?;
    // Deadline 2s out; release deadline is deadline + 5s.
    let deadline = now(&e.worker).await? + 2 * SEC;
    for op in [&e.ops[0], &e.ops[1]] {
        approve_order(&e, op, "o4", &dave, 1_000 * USDC, deadline).await?;
    }
    let order: Value = e.ssl.view("get_order").args_json(json!({ "order_id": "o4" })).await?.json()?;
    advance_past(&e.worker, u(&order["release_deadline"]) as u64).await?;

    send(&e.token, &dave, &e.ssl, 10 * USDC, json!({ "action": "dispute", "order_id": "o4" })).await?;
    let order: Value = e.ssl.view("get_order").args_json(json!({ "order_id": "o4" })).await?.json()?;
    assert_eq!(order["state"], "disputed");

    let id: u64 = e.arbs[0]
        .call(e.ssl.id(), "propose")
        .args_json(json!({ "action": { "type": "resolve_dispute", "order_id": "o4", "upheld": true } }))
        .max_gas()
        .transact()
        .await?
        .into_result()?
        .json()?;
    e.arbs[1].call(e.ssl.id(), "approve_proposal").args_json(json!({ "id": id })).max_gas().transact().await?.into_result()?;

    // 1,000 compensation + the 10 USDC bond back = 100 starting balance - 10 + 1,010.
    assert_eq!(balance(&e.token, dave.id()).await?, 1_100 * USDC);
    let op0: Value = e.ssl.view("get_operator").args_json(json!({ "account_id": e.ops[0].id() })).await?.json()?;
    assert_eq!(u(&op0["bond"]), 1_500 * USDC);
    assert_reserves(&e).await?;
    Ok(())
}
