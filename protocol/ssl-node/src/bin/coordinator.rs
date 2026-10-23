//! ZEC SSL coordinator.
//!
//! Holds no key share and no NEAR key: it quotes, hands out a fresh shielded deposit
//! address per order, asks the operators to approve, and tracks each order to settlement.
//! The vault contract is the source of truth for order state; the coordinator adds what
//! only it sees (deposits as they arrive) and serves the API the ZEC SSL website uses.
//!
//! Underpaid or late deposits are refunded on Zcash: the coordinator builds and proves the
//! refund, then runs a two-round rerandomized FROST signing round with a threshold of
//! operators, each of which checks the transaction for itself before signing.
//!
//! Configuration (environment):
//!   COORDINATOR_LISTEN       127.0.0.1:7000 (or 0.0.0.0:$PORT when PORT is set)
//!   OPERATORS                name=url pairs, comma separated; or OPERATOR_1_URL /
//!                            OPERATOR_1_NAME ... OPERATOR_9_URL (host:port is accepted)
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
//!   FROST_PUBLIC_KEY_PACKAGE the treasury's FROST public key package (hex), or
//!   FROST_CONFIG + FROST_GROUP_KEY   any participant's frost-client config and the group
//!                            (optional: without it the coordinator cannot run refunds)
//!   REFUND_FEE_ZAT           10000, kept back from each refund for the network fee

use std::collections::BTreeMap;
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
use base64::Engine as _;
use ssl_node::signing::{txid_from_sighash, Round1Request, Round1Response, Round2Request, Round2Response, RoundKind, SpendPackage};
use ssl_node::vault::{Vault, VaultConfig};
use ssl_node::{config, now_ns, ns_to_rfc3339, price::PriceFeed, VERSION};
use ssl_policy::{deposit_status, valid_near_account, DepositStatus, OrderTerms, Proposal, SeenDeposit};
use ssl_wallet::{parse_network, TreasuryWallet};
use tokio::sync::Mutex;

/// Indices below this are reserved (0 is the treasury's default address).
const INDEX_MIN: u64 = 1_000;

/// 64 bits from the OS-seeded hasher keys std keeps per thread. Used for uniqueness (order
/// ids, address indices), not for secrecy.
fn random_u64() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    h.finish()
}

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
    frost: Option<ssl_frost::Coordinator>,
    refund_fee_zat: u64,
}

/// How long to wait before trying a failed refund again.
const REFUND_RETRY: Duration = Duration::from_secs(120);

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
    // v2: events carry the order they belong to. Older events are backfilled from their
    // titles, which always name the order ("Order zs-1 opened").
    let has_order_id: bool = conn.prepare("SELECT 1 FROM pragma_table_info('events') WHERE name = 'order_id'")?.exists([])?;
    if !has_order_id {
        conn.execute_batch(
            "ALTER TABLE events ADD COLUMN order_id TEXT;
             UPDATE events SET order_id = CASE
                 WHEN instr(substr(title, instr(title, 'zs-')), ' ') = 0 THEN substr(title, instr(title, 'zs-'))
                 ELSE substr(title, instr(title, 'zs-'), instr(substr(title, instr(title, 'zs-')), ' ') - 1)
               END
             WHERE instr(title, 'zs-') > 0;
             CREATE INDEX IF NOT EXISTS events_order ON events (order_id);",
        )?;
    }
    // v3: refunds and their signing rounds. A round keeps its proven PCZT so a retry signs
    // the same transaction: operators sign only one refund per order.
    let has_refund: bool = conn.prepare("SELECT 1 FROM pragma_table_info('orders') WHERE name = 'refund_tx'")?.exists([])?;
    if !has_refund {
        conn.execute_batch(
            "ALTER TABLE orders ADD COLUMN refund_tx TEXT;
             ALTER TABLE orders ADD COLUMN refund_expiry INTEGER;
             ALTER TABLE orders ADD COLUMN refund_attempt_ns INTEGER;",
        )?;
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS signing_rounds (
            id TEXT PRIMARY KEY,
            order_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            state TEXT NOT NULL,
            signers TEXT NOT NULL DEFAULT '',
            collected INTEGER NOT NULL DEFAULT 0,
            sighash TEXT,
            txid TEXT,
            expiry INTEGER,
            pczt TEXT,
            error TEXT,
            started_ns INTEGER NOT NULL,
            finished_ns INTEGER
        );
        CREATE INDEX IF NOT EXISTS signing_rounds_order ON signing_rounds (order_id);",
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
        "awaiting_deposit" | "deposit_detected" | "confirming" | "signing" | "refunding" | "released" | "refunded" | "expired" | "disputed" | "compensated" => state,
        "needs_refund" => "refunding",
        _ => "quoted",
    }
}

impl Coordinator {
    fn quote(&self, zec_zat: u64, zec_usd: f64) -> u128 {
        let zec = zec_zat as f64 / 1e8;
        let usd = zec * zec_usd * (1.0 - self.spread_bps as f64 / 10_000.0);
        (usd * 10f64.powi(self.vault_config.token_decimals as i32)).floor() as u128
    }

    async fn event(&self, order_id: &str, kind: &str, title: &str, detail: &str) {
        let db = self.db.lock().await;
        if let Err(e) = db.execute(
            "INSERT INTO events (order_id, kind, title, detail, at_ns) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![order_id, kind, title, detail, now_ns() as i64],
        ) {
            tracing::error!(error = %e, "recording an event");
        }
    }

    async fn set_state(&self, id: &str, state: &str) -> Result<bool> {
        let db = self.db.lock().await;
        let changed = db.execute("UPDATE orders SET state = ?1, updated_ns = ?2 WHERE id = ?3 AND state != ?1", params![state, now_ns() as i64, id])?;
        Ok(changed > 0)
    }

    async fn call_operator(&self, op: &Operator, path: &str, body: &Value) -> Result<Value, String> {
        // An approval waits for one NEAR transaction to be final, a few seconds.
        self.call_operator_within(op, path, body, Duration::from_secs(30)).await
    }

    async fn call_operator_within(&self, op: &Operator, path: &str, body: &Value, timeout: Duration) -> Result<Value, String> {
        let res = self
            .http
            .post(format!("{}{path}", op.url))
            .bearer_auth(&self.token)
            .json(body)
            .timeout(timeout)
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
        // Ids and address indices carry randomness, so they never repeat even if this
        // coordinator's state is lost and the sequence restarts: a reused id would ask
        // operators to approve new terms for an existing order (equivocation), and a reused
        // index would hand out an old order's deposit address.
        let id = format!("zs-{seq}-{:06x}", random_u64() & 0xff_ffff);
        // 62 bits keeps the index within SQLite's signed 64-bit integers.
        let index = INDEX_MIN + (random_u64() >> 2);
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
            refund_address: Some(req.refund_address.clone()),
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
            self.event(&id, "order", &format!("Order {id} not opened"), &format!("{approved} of {} operators approved: {}", self.vault_config.threshold, reasons.join("; "))).await;
            return Err((StatusCode::SERVICE_UNAVAILABLE, format!("operators did not approve the order ({approved} of {} needed): {}", self.vault_config.threshold, reasons.join("; "))));
        }
        self.event(&id, "order", &format!("Order {id} opened"), &format!("{zec} ZEC → {out} {} for {}", self.vault_config.token_symbol, req.recipient)).await;
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
                 WHERE state IN ('awaiting_deposit', 'deposit_detected', 'confirming', 'signing', 'needs_refund', 'refunding')",
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
                self.event(id, if final_state == "released" { "release" } else { "order" }, &title, &format!("vault state: {final_state}")).await;
            }
            return Ok(());
        }
        if state == "refunding" {
            return self.watch_refund(id).await;
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
            self.event(id, kind, &title, &format!("{} of {} ZEC received", format_units(received as u128, 8), format_units(zec_zat as u128, 8))).await;
        }
        if next == "needs_refund" {
            if let DepositStatus::Underpaid { received_zat } | DepositStatus::Late { received_zat } = status {
                self.try_refund(id, received_zat).await;
            }
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

    /// A broadcast refund that is never mined expires; the order then goes back to
    /// needs_refund and the operators may sign a replacement.
    async fn watch_refund(&self, id: &str) -> Result<()> {
        let (txid, expiry): (Option<String>, Option<u32>) = {
            let db = self.db.lock().await;
            db.query_row("SELECT refund_tx, refund_expiry FROM orders WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?
        };
        let Some(txid) = txid else { return Ok(()) };
        let (mined, tip) = {
            let w = self.wallet.lock().await;
            (w.tx_mined_height(&txid)?, w.heights()?.1.unwrap_or(0))
        };
        if mined.is_none() && expiry.is_some_and(|e| e > 0 && tip > e) && self.set_state(id, "needs_refund").await? {
            self.event(id, "refund", &format!("Refund for {id} expired unmined"), &format!("{txid} was not mined by block {}; trying again", expiry.unwrap_or(0))).await;
        }
        Ok(())
    }

    /// Refunds an order's deposit, minus the network fee allowance, to the refund address
    /// the operators approved. Failures are recorded and retried after `REFUND_RETRY`.
    async fn try_refund(&self, id: &str, received_zat: u64) {
        let Some(frost) = &self.frost else { return };
        let row: Option<(String, Option<i64>)> = {
            let db = self.db.lock().await;
            db.query_row("SELECT refund_address, refund_attempt_ns FROM orders WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).ok()
        };
        let Some((refund_address, last_attempt)) = row else { return };
        if last_attempt.is_some_and(|t| now_ns().saturating_sub(t as u64) < REFUND_RETRY.as_nanos() as u64) {
            return;
        }
        {
            let db = self.db.lock().await;
            let _ = db.execute("UPDATE orders SET refund_attempt_ns = ?1 WHERE id = ?2", params![now_ns() as i64, id]);
        }
        if received_zat <= self.refund_fee_zat {
            return tracing::warn!(order = id, received_zat, "deposit is too small to refund");
        }
        let round_id = format!("rf-{id}-{:08x}", random_u64() as u32);
        {
            let db = self.db.lock().await;
            let _ = db.execute(
                "INSERT INTO signing_rounds (id, order_id, kind, state, started_ns) VALUES (?1, ?2, 'refund', 'open', ?3)",
                params![round_id, id, now_ns() as i64],
            );
        }
        match self.refund(frost, id, &round_id, &refund_address, received_zat - self.refund_fee_zat).await {
            Ok(txid) => {
                tracing::info!(order = id, round = %round_id, %txid, "refund broadcast");
                self.finish_round(&round_id, "complete", None).await;
                self.set_state(id, "refunding").await.ok();
                self.event(
                    id,
                    "refund",
                    &format!("Refund sent for {id}"),
                    &format!("{} ZEC back to the refund address in {txid}", format_units((received_zat - self.refund_fee_zat) as u128, 8)),
                )
                .await;
            }
            Err(e) => {
                tracing::warn!(order = id, round = %round_id, error = %e, "refund round failed");
                self.finish_round(&round_id, "failed", Some(&e.to_string())).await;
                self.event(id, "signing", &format!("Refund round for {id} failed"), &e.to_string()).await;
            }
        }
    }

    async fn finish_round(&self, round_id: &str, state: &str, error: Option<&str>) {
        let db = self.db.lock().await;
        if let Err(e) = db.execute(
            "UPDATE signing_rounds SET state = ?1, error = ?2, finished_ns = ?3 WHERE id = ?4",
            params![state, error, now_ns() as i64, round_id],
        ) {
            tracing::error!(error = %e, "recording a signing round");
        }
    }

    /// One refund signing round. Returns the broadcast txid.
    async fn refund(&self, frost: &ssl_frost::Coordinator, id: &str, round_id: &str, to: &str, value_zat: u64) -> Result<String> {
        // Reuse the transaction of an earlier attempt while it can still be mined: an
        // operator that already signed it will not sign a different one.
        let tip = self.wallet.lock().await.heights()?.1.unwrap_or(0);
        let earlier: Option<String> = {
            let db = self.db.lock().await;
            db.query_row(
                "SELECT pczt FROM signing_rounds WHERE order_id = ?1 AND pczt IS NOT NULL AND expiry > ?2 ORDER BY started_ns DESC LIMIT 1",
                params![id, tip + 3],
                |r| r.get(0),
            )
            .ok()
        };
        let proved = match earlier {
            Some(b64) => base64::engine::general_purpose::STANDARD.decode(b64)?,
            None => {
                let pczt = self.wallet.lock().await.create_payment(to, value_zat, &format!("zec-ssl refund {id}"))?;
                // Proving takes seconds of CPU and needs no wallet.
                tokio::task::spawn_blocking(move || ssl_wallet::prove(&pczt)).await??
            }
        };
        let request = ssl_sign::prepare(&proved)?;
        anyhow::ensure!(request.effects.is_fully_shielded(), "refund is not fully shielded");
        let txid = txid_from_sighash(&request.sighash).ok_or_else(|| anyhow!("bad sighash"))?;
        let pczt_b64 = base64::engine::general_purpose::STANDARD.encode(&proved);
        {
            let db = self.db.lock().await;
            db.execute(
                "UPDATE signing_rounds SET sighash = ?1, txid = ?2, expiry = ?3, pczt = ?4 WHERE id = ?5",
                params![request.sighash, txid, request.effects.expiry_height, pczt_b64, round_id],
            )?;
        }

        // Round 1: every operator checks the transaction; the first threshold to commit sign.
        let threshold = self.vault_config.threshold as usize;
        let body = serde_json::to_value(Round1Request { round_id: round_id.into(), kind: RoundKind::Refund, order_id: id.into(), pczt: pczt_b64 })?;
        let timeout = Duration::from_secs(90);
        let body = &body;
        let mut pending: FuturesUnordered<_> = self
            .operators
            .iter()
            .map(|op| async move { (op, self.call_operator_within(op, "/v1/signing/round1", &body, timeout).await) })
            .collect();
        let (mut chosen, mut reasons): (Vec<(&Operator, Round1Response)>, Vec<String>) = (vec![], vec![]);
        while let Some((op, result)) = pending.next().await {
            match result.and_then(|v| serde_json::from_value::<Round1Response>(v).map_err(|e| format!("{}: {e}", op.name))) {
                Ok(r) if r.sighash == request.sighash => chosen.push((op, r)),
                Ok(_) => reasons.push(format!("{}: computed a different sighash", op.name)),
                Err(e) => reasons.push(e),
            }
            if chosen.len() >= threshold {
                break;
            }
        }
        drop(pending);
        anyhow::ensure!(chosen.len() >= threshold, "{} of {threshold} operators committed: {}", chosen.len(), reasons.join("; "));
        {
            let names = chosen.iter().map(|(op, _)| op.name.as_str()).collect::<Vec<_>>().join(",");
            let db = self.db.lock().await;
            db.execute("UPDATE signing_rounds SET signers = ?1 WHERE id = ?2", params![names, round_id])?;
        }

        // Round 2: one signing package per spend, from the chosen operators' commitments.
        let message = hex::decode(&request.sighash)?;
        let mut packages = vec![];
        for spend in &request.spends {
            let mut commitments = BTreeMap::new();
            for (op, r) in &chosen {
                let c = r
                    .commitments
                    .iter()
                    .find(|c| c.pool == spend.pool && c.index == spend.index)
                    .ok_or_else(|| anyhow!("{} sent no commitment for a spend", op.name))?;
                commitments.insert(r.identifier.clone(), c.commitments.clone());
            }
            packages.push(SpendPackage { pool: spend.pool, index: spend.index, signing_package: frost.signing_package(&commitments, &message)? });
        }
        let body = serde_json::to_value(Round2Request { round_id: round_id.into(), packages: packages.clone() })?;
        let replies = join_all(chosen.iter().map(|(op, _)| self.call_operator_within(op, "/v1/signing/round2", &body, timeout))).await;
        let mut responses = vec![];
        for (reply, (op, _)) in replies.into_iter().zip(&chosen) {
            let r: Round2Response = serde_json::from_value(reply.map_err(|e| anyhow!(e))?).with_context(|| format!("{}: bad round 2 reply", op.name))?;
            responses.push(r);
        }
        {
            let db = self.db.lock().await;
            db.execute("UPDATE signing_rounds SET collected = ?1 WHERE id = ?2", params![responses.len() as i64, round_id])?;
        }

        // Aggregate (which verifies every share), sign, and send.
        let mut signatures = vec![];
        for (spend, package) in request.spends.iter().zip(&packages) {
            let mut shares = BTreeMap::new();
            for r in &responses {
                let s = r.shares.iter().find(|s| s.pool == spend.pool && s.index == spend.index).ok_or_else(|| anyhow!("missing a signature share"))?;
                shares.insert(r.identifier.clone(), s.share.clone());
            }
            let alpha: [u8; 32] = hex::decode(&spend.alpha)?.try_into().map_err(|_| anyhow!("bad alpha"))?;
            let signature = frost.aggregate(&package.signing_package, &shares, &alpha)?;
            signatures.push(ssl_sign::SpendSignature { pool: spend.pool, index: spend.index, signature });
        }
        let signed = ssl_sign::apply(&proved, &signatures)?;
        let tx = ssl_sign::extract(&signed)?;
        anyhow::ensure!(tx.transparent_inputs == 0 && tx.transparent_outputs == 0, "extracted refund has transparent parts");
        anyhow::ensure!(tx.txid == txid, "extracted txid {} differs from the signed sighash", tx.txid);
        let send = self.wallet.lock().await.broadcast(&tx.raw);
        send.await?;
        {
            let db = self.db.lock().await;
            db.execute(
                "UPDATE orders SET refund_tx = ?1, refund_expiry = ?2, updated_ns = ?3 WHERE id = ?4",
                params![txid, request.effects.expiry_height, now_ns() as i64, id],
            )?;
        }
        Ok(txid)
    }

    async fn order_rows(&self, where_id: Option<&str>, limit: i64) -> Result<Vec<Value>> {
        let db = self.db.lock().await;
        let sql = "SELECT id, state, zec_zat, amount_out, approvals, release_approvals, created_ns, updated_ns, address, deadline_ns, received_zat, deposit_txids, recipient, refund_tx
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
                "refundTxid": r.get::<_, Option<String>>(13)?,
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

fn event_rows(db: &rusqlite::Connection, order_id: Option<&str>, limit: i64) -> Result<Vec<Value>> {
    let mut stmt = db.prepare(
        "SELECT id, kind, title, detail, at_ns, order_id FROM events
         WHERE (?1 IS NULL OR order_id = ?1) ORDER BY id DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![order_id, limit], |r| {
        Ok(json!({
            "id": r.get::<_, i64>(0)?.to_string(),
            "kind": r.get::<_, String>(1)?,
            "title": r.get::<_, String>(2)?,
            "detail": r.get::<_, String>(3)?,
            "at": ns_to_rfc3339(r.get::<_, i64>(4)? as u64),
            "orderId": r.get::<_, Option<String>>(5)?,
        }))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

async fn events(State(c): State<App>, Query(q): Query<Limit>) -> Response {
    let db = c.db.lock().await;
    match event_rows(&db, None, q.limit.unwrap_or(50).clamp(1, 200)) {
        Ok(v) => Json(json!(v)).into_response(),
        Err(e) => err_json(internal(e)),
    }
}

async fn order_events(State(c): State<App>, Path(id): Path<String>) -> Response {
    let db = c.db.lock().await;
    match event_rows(&db, Some(&id), 200) {
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

async fn signing_rounds(State(c): State<App>, Query(q): Query<Limit>) -> Response {
    let db = c.db.lock().await;
    let rows = (|| -> Result<Vec<Value>> {
        let mut stmt = db.prepare(
            "SELECT id, order_id, kind, state, signers, collected, started_ns, finished_ns, txid, error
             FROM signing_rounds ORDER BY started_ns DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([q.limit.unwrap_or(50).clamp(1, 200)], |r| {
            let signers: String = r.get(4)?;
            Ok(json!({
                "id": r.get::<_, String>(0)?,
                "orderId": r.get::<_, String>(1)?,
                "kind": r.get::<_, String>(2)?,
                "state": r.get::<_, String>(3)?,
                "signers": signers.split(',').filter(|s| !s.is_empty()).collect::<Vec<_>>(),
                "collected": r.get::<_, i64>(5)?,
                "threshold": c.vault_config.threshold,
                "startedAt": ns_to_rfc3339(r.get::<_, i64>(6)? as u64),
                "completedAt": r.get::<_, Option<i64>>(7)?.map(|t| ns_to_rfc3339(t as u64)),
                "txid": r.get::<_, Option<String>>(8)?,
                "error": r.get::<_, Option<String>>(9)?,
            }))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    })();
    match rows {
        Ok(v) => Json(json!(v)).into_response(),
        Err(e) => err_json(internal(e)),
    }
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

    let listen: SocketAddr = config::listen_addr("COORDINATOR_LISTEN", 7000)?;
    let operators: Vec<Operator> = config::endpoints("OPERATORS", "OPERATOR")?.into_iter().map(|(name, url)| Operator { name, url }).collect();
    anyhow::ensure!(!operators.is_empty(), "set OPERATORS or OPERATOR_1_URL ... OPERATOR_N_URL");
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

    let frost = match (std::env::var("FROST_PUBLIC_KEY_PACKAGE").ok().filter(|v| !v.is_empty()), std::env::var("FROST_CONFIG").ok().filter(|v| !v.is_empty())) {
        (Some(pkg), _) => Some(ssl_frost::Coordinator::from_hex(&pkg)?),
        (None, Some(path)) => Some(ssl_frost::Coordinator::from_frost_client_config(path.as_ref(), &config::req("FROST_GROUP_KEY")?)?),
        (None, None) => None,
    };
    match &frost {
        Some(f) => tracing::info!(group = %f.group_key_hex(), "FROST public key package loaded"),
        None => tracing::warn!("no FROST public key package configured; refunds will not be signed"),
    }
    let refund_fee_zat: u64 = config::num("REFUND_FEE_ZAT", 10_000)?;

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
        frost,
        refund_fee_zat,
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
        .route("/v1/orders/{id}/events", get(order_events))
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
