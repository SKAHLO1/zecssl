//! ZEC SSL coordinator.
//!
//! Holds no key share and no NEAR key: it quotes, hands out a fresh shielded deposit
//! address per order, asks the operators to approve, and tracks each order to settlement.
//! The vault contract is the source of truth for order state; the coordinator adds what
//! only it sees (deposits as they arrive) and serves the API the ZEC SSL website uses.
//!
//! Configuration (environment):
//!   COORDINATOR_LISTEN       127.0.0.1:7000
//!   OPERATORS                name=url pairs, comma separated (required)
//!   OPERATOR_API_TOKEN       bearer token for the operators (required)
//!   NEAR_RPC_URL             https://rpc.testnet.fastnear.com
//!   NEAR_VAULT_CONTRACT      the vault account (required)
//!   ZCASH_NETWORK            test
//!   LIGHTWALLETD_URL         https://testnet.zec.rocks:443
//!   TREASURY_UFVK            the treasury viewing key, or @/path/to/file (required)
//!   WALLET_BIRTHDAY          block height to scan from on first start
//!   STATE_DIR                ./coordinator-state
//!   CONFIRMATIONS            12
//!   SPREAD_BPS               25
//!   DEPOSIT_WINDOW_MINS      30
//!   MAX_ORDER_USDC           1000
//!   MIN_ORDER_ZEC            0.001
//!   SYNC_INTERVAL_SECS       20

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use futures::future::join_all;
use futures::stream::{FuturesUnordered, StreamExt};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rusqlite::params;
use serde::Deserialize;
use serde_json::{json, Value};
use ssl_node::vault::{Vault, VaultConfig};
use ssl_node::{config, now_ns, ns_to_rfc3339, price::PriceFeed, VERSION};
use ssl_policy::{deposit_status, valid_near_account, DepositStatus, OrderTerms, Proposal, SeenDeposit};
use ssl_wallet::{parse_network, TreasuryWallet};
use tokio::sync::Mutex;

const INDEX_BASE: u32 = 1_000;

struct Operator {
    name: String,
    url: String,
}

struct Coordinator {
    vault: Vault,
    vault_config: VaultConfig,
    wallet: Mutex<TreasuryWallet>,
    db: Mutex<rusqlite::Connection>,
    prices: PriceFeed,
    operators: Vec<Operator>,
    token: String,
    http: reqwest::Client,
    confirmations: u32,
    spread_bps: u32,
    deposit_window_ns: u64,
    max_amount_out: u128,
    min_zec_zat: u64,
    last_sync: Mutex<(Option<u64>, Option<String>)>,
}

fn open_db(path: &PathBuf) -> Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS orders (
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            id TEXT UNIQUE,
            idx INTEGER,
            address TEXT,
            recipient TEXT NOT NULL,
            refund_address TEXT NOT NULL,
            amount_out TEXT NOT NULL,
            zec_zat INTEGER NOT NULL,
            deadline_ns INTEGER NOT NULL,
            zec_usd REAL NOT NULL,
            state TEXT NOT NULL,
            approvals INTEGER NOT NULL DEFAULT 0,
            release_approvals INTEGER NOT NULL DEFAULT 0,
            received_zat INTEGER NOT NULL DEFAULT 0,
            deposit_txids TEXT,
            created_ns INTEGER NOT NULL,
            updated_ns INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL,
            title TEXT NOT NULL,
            detail TEXT NOT NULL,
            at_ns INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS deposits_seen (
            txid TEXT PRIMARY KEY,
            first_seen_mined_ns INTEGER NOT NULL
        );",
    )?;
    Ok(conn)
}

/// Parses a decimal ZEC amount into zatoshis without floating point.
fn parse_zec(s: &str) -> Result<u64> {
    let s = s.trim();
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    if whole.is_empty() && frac.is_empty() || !whole.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit()) || frac.len() > 8 {
        bail!("amount must be a decimal ZEC value with at most 8 decimals");
    }
    let whole: u64 = if whole.is_empty() { 0 } else { whole.parse()? };
    let frac: u64 = format!("{frac:0<8}").parse()?;
    whole.checked_mul(100_000_000).and_then(|w| w.checked_add(frac)).ok_or_else(|| anyhow!("amount too large"))
}

fn format_units(raw: u128, decimals: u8) -> String {
    let base = 10u128.pow(decimals as u32);
    let frac = raw % base;
    if frac == 0 {
        return (raw / base).to_string();
    }
    format!("{}.{}", raw / base, format!("{frac:0width$}", width = decimals as usize).trim_end_matches('0'))
}

/// Refunds go back into the shielded pool, so only unified addresses are accepted.
fn valid_refund_address(network_test: bool, a: &str) -> bool {
    let prefix = if network_test { "utest1" } else { "u1" };
    a.starts_with(prefix) && a.len() >= 60 && a.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// The website's order states (server/src/types.ts).
fn public_state(state: &str) -> &str {
    match state {
        "awaiting_deposit" | "deposit_detected" | "confirming" | "signing" | "released" | "refunded" | "expired" | "disputed" | "compensated" => state,
        "needs_refund" => "confirming",
        _ => "quoted",
    }
}

impl Coordinator {
    fn quote(&self, zec_zat: u64, zec_usd: f64) -> u128 {
        let zec = zec_zat as f64 / 1e8;
        let usd = zec * zec_usd * (1.0 - self.spread_bps as f64 / 10_000.0);
        (usd * 10f64.powi(self.vault_config.token_decimals as i32)).floor() as u128
    }

    async fn event(&self, kind: &str, title: &str, detail: &str) {
        let db = self.db.lock().await;
        if let Err(e) = db.execute("INSERT INTO events (kind, title, detail, at_ns) VALUES (?1, ?2, ?3, ?4)", params![kind, title, detail, now_ns() as i64]) {
            tracing::error!(error = %e, "recording an event");
        }
    }

    async fn set_state(&self, id: &str, state: &str) -> Result<bool> {
        let db = self.db.lock().await;
        let changed = db.execute("UPDATE orders SET state = ?1, updated_ns = ?2 WHERE id = ?3 AND state != ?1", params![state, now_ns() as i64, id])?;
        Ok(changed > 0)
    }

    async fn call_operator(&self, op: &Operator, path: &str, body: &Value) -> Result<Value, String> {
        let res = self
            .http
            .post(format!("{}{path}", op.url))
            .bearer_auth(&self.token)
            .json(body)
            // An approval waits for one NEAR transaction to be final, a few seconds.
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| format!("{}: unreachable ({e})", op.name))?;
        let status = res.status();
        let v: Value = res.json().await.unwrap_or(Value::Null);
        if status.is_success() {
            Ok(v)
        } else {
            Err(format!("{}: {}", op.name, v["error"].as_str().unwrap_or(status.as_str())))
        }
    }

    /// Creates an order, proposes it to every operator, and returns it once the vault has
    /// opened it. Nothing is returned to the user unless the order is open on chain.
    async fn create_order(&self, req: CreateOrder) -> Result<Value, (StatusCode, String)> {
        let bad = |m: &str| (StatusCode::BAD_REQUEST, m.to_string());
        let zec_zat = parse_zec(&req.amount_zec).map_err(|e| bad(&e.to_string()))?;
        if zec_zat < self.min_zec_zat {
            return Err(bad("amount is below the minimum"));
        }
        if !valid_near_account(&req.recipient) {
            return Err(bad("recipient is not a valid NEAR account"));
        }
        let is_test = self.wallet.lock().await.is_testnet();
        if !valid_refund_address(is_test, &req.refund_address) {
            return Err(bad("refund address must be a shielded unified address"));
        }
        let zec_usd = self.prices.zec_usd().await.ok_or((StatusCode::SERVICE_UNAVAILABLE, "no ZEC price available".into()))?;
        let amount_out = self.quote(zec_zat, zec_usd);
        if amount_out > self.max_amount_out {
            return Err(bad("amount is above the maximum order size"));
        }

        let now = now_ns();
        let deadline = now + self.deposit_window_ns;
        let seq: i64 = {
            let db = self.db.lock().await;
            db.execute(
                "INSERT INTO orders (recipient, refund_address, amount_out, zec_zat, deadline_ns, zec_usd, state, created_ns, updated_ns)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'proposing', ?7, ?7)",
                params![req.recipient, req.refund_address, amount_out.to_string(), zec_zat as i64, deadline as i64, zec_usd, now as i64],
            )
            .map_err(internal)?;
            db.last_insert_rowid()
        };
        let id = format!("zs-{seq}");
        let index = INDEX_BASE + seq as u32;
        let address = self.wallet.lock().await.address(index).map_err(internal)?;
        {
            let db = self.db.lock().await;
            db.execute("UPDATE orders SET id = ?1, idx = ?2, address = ?3 WHERE seq = ?4", params![id, index, address, seq]).map_err(internal)?;
        }

        let proposal = Proposal {
            order_id: id.clone(),
            terms: OrderTerms { recipient: req.recipient.clone(), amount_out, zec_amount_zat: zec_zat, deposit_deadline: deadline },
            diversifier_index: index,
            deposit_address: address.clone(),
            quoted_zec_usd: zec_usd,
        };
        let body = serde_json::to_value(&proposal).map_err(internal)?;
        // Return as soon as the threshold has approved: an operator that is down or hung
        // must not hold up the order. Requests still in flight are dropped; an operator
        // that was mid-approval may still land its approval, which the vault accepts.
        let threshold = self.vault_config.threshold as usize;
        let mut pending: FuturesUnordered<_> = self.operators.iter().map(|op| self.call_operator(op, "/v1/proposals", &body)).collect();
        let (mut approved, mut reasons) = (0usize, vec![]);
        while let Some(result) = pending.next().await {
            match result {
                Ok(_) => approved += 1,
                Err(e) => reasons.push(e),
            }
            if approved >= threshold {
                break;
            }
        }
        drop(pending);

        let on_chain = self.vault.order(&id).await.map_err(internal)?;
        let open = on_chain.as_ref().is_some_and(|o| o.state == "open");
        {
            let db = self.db.lock().await;
            db.execute(
                "UPDATE orders SET state = ?1, approvals = ?2, updated_ns = ?3 WHERE id = ?4",
                params![if open { "awaiting_deposit" } else { "rejected" }, on_chain.as_ref().map_or(0, |o| o.approvers.len()) as i64, now_ns() as i64, id],
            )
            .map_err(internal)?;
        }
        let zec = format_units(zec_zat as u128, 8);
        let out = format_units(amount_out, self.vault_config.token_decimals);
        if !open {
            self.event("order", &format!("Order {id} not opened"), &format!("{approved} of {} operators approved: {}", self.vault_config.threshold, reasons.join("; "))).await;
            return Err((StatusCode::SERVICE_UNAVAILABLE, format!("operators did not approve the order ({approved} of {} needed): {}", self.vault_config.threshold, reasons.join("; "))));
        }
        self.event("order", &format!("Order {id} opened"), &format!("{zec} ZEC → {out} {} for {}", self.vault_config.token_symbol, req.recipient)).await;
        Ok(json!({
            "id": id,
            "state": "awaiting_deposit",
            "depositAddress": address,
            "memo": id,
            "expiresAt": ns_to_rfc3339(deadline),
            "amountZec": zec,
            "amountOut": out,
            "symbol": self.vault_config.token_symbol,
            "zip321": format!("zcash:{address}?amount={zec}&memo={}", base64url(id.as_bytes())),
        }))
    }

    /// One pass: sync the wallet, then advance every open order.
    async fn tick(&self) {
        let synced = { self.wallet.lock().await.sync().await };
        *self.last_sync.lock().await = match &synced {
            Ok(()) => (Some(now_ns()), None),
            Err(e) => (self.last_sync.lock().await.0, Some(e.to_string())),
        };
        if let Err(e) = synced {
            return tracing::warn!(error = %e, "sync failed");
        }
        let open: Vec<(String, String, u64, u64, String)> = {
            let db = self.db.lock().await;
            let mut stmt = match db.prepare(
                "SELECT id, address, zec_zat, deadline_ns, state FROM orders
                 WHERE state IN ('awaiting_deposit', 'deposit_detected', 'confirming', 'signing', 'needs_refund')",
            ) {
                Ok(s) => s,
                Err(e) => return tracing::error!(error = %e, "listing orders"),
            };
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? as u64, r.get::<_, i64>(3)? as u64, r.get(4)?)))
                .map(|rows| rows.filter_map(Result::ok).collect())
                .unwrap_or_default()
        };
        for (id, address, zec_zat, deadline, state) in open {
            if let Err(e) = self.advance(&id, &address, zec_zat, deadline, &state).await {
                tracing::warn!(order = %id, error = %e, "advancing order");
            }
        }
    }

    async fn advance(&self, id: &str, address: &str, zec_zat: u64, deadline: u64, state: &str) -> Result<()> {
        let Some(order) = self.vault.order(id).await? else { return Ok(()) };
        {
            let db = self.db.lock().await;
            db.execute(
                "UPDATE orders SET approvals = ?1, release_approvals = ?2 WHERE id = ?3",
                params![order.approvers.len() as i64, order.release_approvals.len() as i64, id],
            )?;
        }
        if order.state != "open" {
            let final_state = match order.state.as_str() {
                "released" | "refunded" | "expired" | "disputed" | "compensated" => order.state.as_str(),
                _ => return Ok(()),
            };
            if self.set_state(id, final_state).await? {
                let title = match final_state {
                    "released" => format!("Order {id} settled"),
                    "refunded" => format!("Order {id} refunded"),
                    other => format!("Order {id} {other}"),
                };
                self.event(if final_state == "released" { "release" } else { "order" }, &title, &format!("vault state: {final_state}")).await;
            }
            return Ok(());
        }

        let (deposits, tip) = {
            let w = self.wallet.lock().await;
            (w.deposits_to(address)?, w.heights()?.1.unwrap_or(0))
        };
        let seen: Vec<SeenDeposit> = {
            let db = self.db.lock().await;
            let mut seen = vec![];
            for d in deposits {
                let first = if d.mined_height.is_some() {
                    db.execute("INSERT OR IGNORE INTO deposits_seen (txid, first_seen_mined_ns) VALUES (?1, ?2)", params![d.txid, now_ns() as i64])?;
                    Some(db.query_row("SELECT first_seen_mined_ns FROM deposits_seen WHERE txid = ?1", [&d.txid], |r| r.get::<_, i64>(0))? as u64)
                } else {
                    None
                };
                seen.push(SeenDeposit { txid: d.txid, mined_height: d.mined_height, value_zat: d.value_zat, first_seen_mined_ns: first });
            }
            seen
        };
        let status = deposit_status(&seen, zec_zat, tip, self.confirmations, deadline);
        let received: u64 = seen.iter().map(|d| d.value_zat).sum();
        {
            let db = self.db.lock().await;
            let txids = seen.iter().map(|d| d.txid.as_str()).collect::<Vec<_>>().join(",");
            db.execute("UPDATE orders SET received_zat = ?1, deposit_txids = ?2 WHERE id = ?3", params![received as i64, txids, id])?;
        }
        let next = match &status {
            DepositStatus::None => "awaiting_deposit",
            DepositStatus::Confirming { confirmations: 0, .. } => "deposit_detected",
            DepositStatus::Confirming { .. } => "confirming",
            DepositStatus::Paid { .. } => "signing",
            // Underpaid before the deadline may still be topped up.
            DepositStatus::Underpaid { .. } if now_ns() <= deadline => "confirming",
            DepositStatus::Underpaid { .. } | DepositStatus::Late { .. } => "needs_refund",
        };
        if next != state && self.set_state(id, next).await? {
            let (kind, title) = match next {
                "deposit_detected" => ("deposit", format!("Deposit seen for {id}")),
                "confirming" => ("deposit", format!("Deposit confirming for {id}")),
                "signing" => ("deposit", format!("Deposit confirmed for {id}")),
                "needs_refund" => ("refund", format!("Order {id} needs a refund")),
                _ => ("order", format!("Order {id}: {next}")),
            };
            self.event(kind, &title, &format!("{} of {} ZEC received", format_units(received as u128, 8), format_units(zec_zat as u128, 8))).await;
        }
        if matches!(status, DepositStatus::Paid { .. }) {
            // Each operator decides for itself; this only asks them to look now.
            let body = json!({ "order_id": id });
            for r in join_all(self.operators.iter().map(|op| self.call_operator(op, "/v1/releases", &body))).await {
                if let Err(e) = r {
                    tracing::warn!(order = id, error = %e, "release request");
                }
            }
        }
        Ok(())
    }

    async fn order_rows(&self, where_id: Option<&str>, limit: i64) -> Result<Vec<Value>> {
        let db = self.db.lock().await;
        let sql = "SELECT id, state, zec_zat, amount_out, approvals, release_approvals, created_ns, updated_ns, address, deadline_ns, received_zat, deposit_txids, recipient
                   FROM orders WHERE id IS NOT NULL AND state != 'proposing' AND (?1 IS NULL OR id = ?1) ORDER BY seq DESC LIMIT ?2";
        let mut stmt = db.prepare(sql)?;
        let decimals = self.vault_config.token_decimals;
        let t = self.vault_config.threshold;
        let rows = stmt.query_map(params![where_id, limit], |r| {
            let state: String = r.get(1)?;
            let amount_out: String = r.get(3)?;
            let signing = matches!(state.as_str(), "signing" | "released");
            Ok(json!({
                "id": r.get::<_, String>(0)?,
                "direction": "zec_to_near",
                "state": public_state(&state),
                "internalState": state,
                "amountInZec": format_units(r.get::<_, i64>(2)? as u128, 8),
                "amountOut": format_units(amount_out.parse().unwrap_or(0), decimals),
                "destinationSymbol": self.vault_config.token_symbol,
                "signatures": if signing { r.get::<_, i64>(5)? } else { r.get::<_, i64>(4)? },
                "threshold": t,
                "createdAt": ns_to_rfc3339(r.get::<_, i64>(6)? as u64),
                "updatedAt": ns_to_rfc3339(r.get::<_, i64>(7)? as u64),
                "nearTxHash": Value::Null,
                "depositAddress": r.get::<_, Option<String>>(8)?,
                "expiresAt": ns_to_rfc3339(r.get::<_, i64>(9)? as u64),
                "receivedZec": format_units(r.get::<_, i64>(10)? as u128, 8),
                "depositTxids": r.get::<_, Option<String>>(11)?.map(|s| s.split(',').filter(|t| !t.is_empty()).map(str::to_string).collect::<Vec<_>>()).unwrap_or_default(),
                "recipient": r.get::<_, String>(12)?,
            }))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

fn base64url(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |acc, (i, b)| acc | (*b as u32) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(A[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

fn internal(e: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn err_json((code, msg): (StatusCode, String)) -> Response {
    (code, Json(json!({ "error": msg }))).into_response()
}

type App = Arc<Coordinator>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateOrder {
    amount_zec: String,
    recipient: String,
    refund_address: String,
}

async fn create_order(State(c): State<App>, Json(req): Json<CreateOrder>) -> Response {
    match c.create_order(req).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err_json(e),
    }
}

#[derive(Deserialize)]
struct Limit {
    limit: Option<i64>,
}

async fn list_orders(State(c): State<App>, Query(q): Query<Limit>) -> Response {
    match c.order_rows(None, q.limit.unwrap_or(50).clamp(1, 200)).await {
        Ok(v) => Json(json!(v)).into_response(),
        Err(e) => err_json(internal(e)),
    }
}

async fn get_order(State(c): State<App>, Path(id): Path<String>) -> Response {
    match c.order_rows(Some(&id), 1).await {
        Ok(v) if !v.is_empty() => Json(v[0].clone()).into_response(),
        Ok(_) => err_json((StatusCode::NOT_FOUND, "unknown order".into())),
        Err(e) => err_json(internal(e)),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuoteQuery {
    amount_zec: String,
}

async fn quote(State(c): State<App>, Query(q): Query<QuoteQuery>) -> Response {
    let Ok(zat) = parse_zec(&q.amount_zec) else { return err_json((StatusCode::BAD_REQUEST, "invalid amount".into())) };
    let Some(price) = c.prices.zec_usd().await else { return err_json((StatusCode::SERVICE_UNAVAILABLE, "no ZEC price available".into())) };
    let out = c.quote(zat, price);
    let d = c.vault_config.token_decimals;
    Json(json!({
        "amountIn": format_units(zat as u128, 8),
        "amountOut": format_units(out, d),
        "amountOutRaw": out.to_string(),
        "symbol": c.vault_config.token_symbol,
        "decimals": d,
        "zecUsd": price,
        "rate": out as f64 / 10f64.powi(d as i32) / (zat as f64 / 1e8),
        "spreadBps": c.spread_bps,
        "maxAmountOut": format_units(c.max_amount_out, d),
        "minAmountZec": format_units(c.min_zec_zat as u128, 8),
        "depositWindowSecs": c.deposit_window_ns / 1_000_000_000,
        "confirmations": c.confirmations,
    }))
    .into_response()
}

async fn events(State(c): State<App>, Query(q): Query<Limit>) -> Response {
    let db = c.db.lock().await;
    let rows: Result<Vec<Value>> = (|| {
        let mut stmt = db.prepare("SELECT id, kind, title, detail, at_ns FROM events ORDER BY id DESC LIMIT ?1")?;
        let rows = stmt.query_map([q.limit.unwrap_or(50).clamp(1, 200)], |r| {
            Ok(json!({ "id": r.get::<_, i64>(0)?.to_string(), "kind": r.get::<_, String>(1)?, "title": r.get::<_, String>(2)?, "detail": r.get::<_, String>(3)?, "at": ns_to_rfc3339(r.get::<_, i64>(4)? as u64) }))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    })();
    match rows {
        Ok(v) => Json(json!(v)).into_response(),
        Err(e) => err_json(internal(e)),
    }
}

async fn treasury(State(c): State<App>) -> Response {
    let w = c.wallet.lock().await;
    let issued: i64 = c.db.lock().await.query_row("SELECT COUNT(*) FROM orders WHERE address IS NOT NULL", [], |r| r.get(0)).unwrap_or(0);
    match (w.balance_zat(), w.heights()) {
        (Ok(bal), Ok((_, scanned))) => Json(json!({ "shieldedBalanceZat": bal.to_string(), "addressesIssued": issued, "lastScanHeight": scanned })).into_response(),
        (Err(e), _) | (_, Err(e)) => err_json(internal(e)),
    }
}

async fn signing_rounds() -> Json<Value> {
    // Direction-1 settlement needs no Zcash signatures; FROST rounds arrive with refunds.
    Json(json!([]))
}

async fn health(State(c): State<App>) -> Json<Value> {
    let (last_ok, err) = c.last_sync.lock().await.clone();
    let fresh = last_ok.is_some_and(|t| now_ns().saturating_sub(t) < 120_000_000_000);
    Json(json!({ "status": if fresh && err.is_none() { "ok" } else { "degraded" }, "version": VERSION, "lastSyncAt": last_ok.map(ns_to_rfc3339), "lastError": err }))
}

#[tokio::main]
async fn main() -> Result<()> {
    ssl_node::init_tracing();
    ssl_wallet::install_crypto_provider();

    let listen: SocketAddr = config::opt("COORDINATOR_LISTEN", "127.0.0.1:7000").parse()?;
    let operators: Vec<Operator> = config::req("OPERATORS")?
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|pair| {
            let (name, url) = pair.split_once('=').ok_or_else(|| anyhow!("OPERATORS entries are name=url"))?;
            Ok(Operator { name: name.trim().into(), url: url.trim().trim_end_matches('/').into() })
        })
        .collect::<Result<_>>()?;
    let vault = Vault::new(&config::opt("NEAR_RPC_URL", "https://rpc.testnet.fastnear.com"), &config::req("NEAR_VAULT_CONTRACT")?);
    let vault_config = vault.config().await.context("reading the vault config")?;
    anyhow::ensure!(operators.len() >= vault_config.threshold as usize, "fewer operator endpoints than the vault threshold");

    let network = parse_network(&config::opt("ZCASH_NETWORK", "test"))?;
    let state_dir = PathBuf::from(config::opt("STATE_DIR", "./coordinator-state"));
    std::fs::create_dir_all(&state_dir)?;
    let wallet = TreasuryWallet::open(
        &state_dir.join("wallet"),
        network,
        &config::value_or_file("TREASURY_UFVK")?,
        &config::opt("LIGHTWALLETD_URL", "https://testnet.zec.rocks:443"),
        std::env::var("WALLET_BIRTHDAY").ok().and_then(|v| v.parse().ok()),
    )
    .await?;

    let decimals = vault_config.token_decimals;
    let coordinator = Arc::new(Coordinator {
        vault,
        wallet: Mutex::new(wallet),
        db: Mutex::new(open_db(&state_dir.join("coordinator.sqlite"))?),
        prices: PriceFeed::coingecko(),
        operators,
        token: config::req("OPERATOR_API_TOKEN")?,
        http: reqwest::Client::new(),
        confirmations: config::num("CONFIRMATIONS", 12)?,
        spread_bps: config::num("SPREAD_BPS", 25)?,
        deposit_window_ns: config::num::<u64>("DEPOSIT_WINDOW_MINS", 30)? * 60 * 1_000_000_000,
        max_amount_out: config::num::<u128>("MAX_ORDER_USDC", 1_000)? * 10u128.pow(decimals as u32),
        min_zec_zat: (config::num::<f64>("MIN_ORDER_ZEC", 0.001)? * 1e8) as u64,
        vault_config,
        last_sync: Mutex::new((None, None)),
    });
    tracing::info!(vault = %coordinator.vault.contract, operators = coordinator.operators.len(), %listen, "coordinator starting");

    let interval = Duration::from_secs(config::num("SYNC_INTERVAL_SECS", 20)?);
    let ticker = coordinator.clone();
    tokio::spawn(async move {
        loop {
            ticker.tick().await;
            tokio::time::sleep(interval).await;
        }
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/quote", get(quote))
        .route("/v1/orders", post(create_order).get(list_orders))
        .route("/v1/orders/{id}", get(get_order))
        .route("/v1/events", get(events))
        .route("/v1/treasury", get(treasury))
        .route("/v1/signing-rounds", get(signing_rounds))
        .with_state(coordinator);
    let listener = tokio::net::TcpListener::bind(listen).await?;
    axum::serve(listener, app).with_graceful_shutdown(async { let _ = tokio::signal::ctrl_c().await; }).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_zec_amounts_exactly() {
        assert_eq!(parse_zec("1").unwrap(), 100_000_000);
        assert_eq!(parse_zec("0.001").unwrap(), 100_000);
        assert_eq!(parse_zec(".5").unwrap(), 50_000_000);
        assert_eq!(parse_zec("12.34567891").unwrap(), 1_234_567_891);
        assert!(parse_zec("1.123456789").is_err());
        assert!(parse_zec("1e3").is_err());
        assert!(parse_zec("").is_err());
    }

    #[test]
    fn base64url_encodes_without_padding() {
        assert_eq!(base64url(b"zs-1"), "enMtMQ");
        assert_eq!(base64url(b"zs-12"), "enMtMTI");
        assert_eq!(base64url(b"zs-123"), "enMtMTIz");
    }

    #[test]
    fn formats_units() {
        assert_eq!(format_units(123_450_000, 6), "123.45");
        assert_eq!(format_units(100_000, 8), "0.001");
    }
}
