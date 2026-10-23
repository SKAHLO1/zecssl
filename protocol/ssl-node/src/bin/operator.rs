//! ZEC SSL operator node.
//!
//! An operator holds its own NEAR key and runs its own view-only treasury wallet. It:
//! - checks each order the coordinator proposes against its own rules, price feed and
//!   address derivation, and approves it on the NEAR vault only if every check passes;
//! - watches its own wallet for each approved order's deposit and approves the release
//!   once the deposit is paid in full, confirmed, and on time;
//! - signs refunds of underpaid or late deposits in a rerandomized FROST round, but only
//!   after checking the transaction itself (see `ssl_policy::check_refund`), and approves
//!   the refund on the vault once its own wallet sees it mined;
//! - reports its health at `GET /health`.
//!
//! It never trusts the coordinator: a proposal is just a request to check.
//!
//! Configuration (environment):
//!   OPERATOR_LISTEN          127.0.0.1:7101 (or 0.0.0.0:$PORT when PORT is set)
//!   OPERATOR_API_TOKEN       bearer token the coordinator must present (required)
//!   NEAR_RPC_URL             https://rpc.testnet.fastnear.com
//!   NEAR_VAULT_CONTRACT      the vault account (required)
//!   NEAR_CREDENTIALS         path to this operator's NEAR CLI credentials file, or
//!   NEAR_ACCOUNT_ID + NEAR_SECRET_KEY   the account and its ed25519 secret key
//!   ZCASH_NETWORK            test
//!   LIGHTWALLETD_URL         https://testnet.zec.rocks:443
//!   TREASURY_UFVK            the treasury viewing key, or @/path/to/file (required)
//!   WALLET_BIRTHDAY          block height to scan from on first start
//!   STATE_DIR                ./operator-state
//!   CONFIRMATIONS            12
//!   SPREAD_BPS               25
//!   PRICE_TOLERANCE_BPS      200
//!   MAX_ORDER_USDC           1000
//!   MIN_ORDER_ZEC            0.001
//!   SYNC_INTERVAL_SECS       20
//!   FROST_KEY_PACKAGE + FROST_PUBLIC_KEY_PACKAGE   this operator's FROST share (hex), or
//!   FROST_CONFIG + FROST_GROUP_KEY                 a frost-client config file and group
//!                            (optional: without a share the operator cannot sign refunds)
//!   MAX_REFUND_FEE_ZAT       20000

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use ssl_near::Signer;
use base64::Engine as _;
use ssl_frost::{Share, SigningNonces};
use ssl_node::signing::{refund_tx, txid_from_sighash, Round1Request, Round1Response, Round2Request, Round2Response, SpendCommitment, SpendShare};
use ssl_node::{config, now_ns, ns_to_rfc3339, price::PriceFeed, vault::Vault, VERSION};
use ssl_policy::{check_against_vault, check_proposal, check_refund, deposit_status, DepositStatus, Limits, Proposal, SeenDeposit};
use ssl_wallet::{derive_address, parse_network, TreasuryWallet};
use tokio::sync::Mutex;

struct Node {
    signer: Signer,
    vault: Vault,
    wallet: Mutex<TreasuryWallet>,
    db: Mutex<rusqlite::Connection>,
    prices: PriceFeed,
    limits: Limits,
    confirmations: u32,
    token: String,
    status: Mutex<Status>,
    bond_cache: Mutex<Option<(Option<f64>, std::time::Instant)>>,
    frost: Option<Share>,
    /// Open signing rounds. Nonces live only here, in memory, are used once, and expire.
    rounds: Mutex<HashMap<String, OpenRound>>,
    /// Held while deciding whether to release signature shares, so two rounds for one
    /// order cannot both pass the one-refund check.
    signing: Mutex<()>,
    max_refund_fee_zat: u64,
}

struct OpenRound {
    order_id: String,
    sighash: Vec<u8>,
    expiry_height: u32,
    /// (pool, action index, alpha, nonces) per spend.
    spends: Vec<(ssl_sign::Pool, usize, [u8; 32], SigningNonces)>,
    opened: std::time::Instant,
}

const ROUND_TTL: Duration = Duration::from_secs(600);
/// A refund must expire soon enough that, if it is never mined, a replacement can be signed.
const MAX_REFUND_EXPIRY_BLOCKS: u32 = 100;

#[derive(Default)]
struct Status {
    last_sync_ok: Option<u64>,
    last_error: Option<String>,
    last_signature_at: Option<u64>,
}

fn open_db(path: &PathBuf) -> Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS orders (
            id TEXT PRIMARY KEY,
            proposal TEXT NOT NULL,
            approved_tx TEXT,
            release_tx TEXT,
            final_state TEXT,
            created_ns INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS deposits_seen (
            txid TEXT PRIMARY KEY,
            first_seen_mined_ns INTEGER NOT NULL
        );",
    )?;
    // v2: refunds. `refund_tx` is the Zcash txid this operator signed for the order's refund,
    // `refund_expiry` the last block it can be mined in, and `refund_approval_tx` the NEAR
    // approve_refund.
    let has_refund = conn.prepare("SELECT 1 FROM pragma_table_info('orders') WHERE name = 'refund_tx'")?.exists([])?;
    if !has_refund {
        conn.execute_batch("ALTER TABLE orders ADD COLUMN refund_tx TEXT;
             ALTER TABLE orders ADD COLUMN refund_expiry INTEGER;
             ALTER TABLE orders ADD COLUMN refund_approval_tx TEXT;")?;
    }
    Ok(conn)
}

impl Node {
    /// Checks a proposal and, if every rule passes, approves it on the vault.
    async fn handle_proposal(&self, p: Proposal) -> Result<Value, (StatusCode, String)> {
        let derived = {
            // Register the address in our wallet too, so deposits to it are attributed.
            let mut w = self.wallet.lock().await;
            let derived = derive_address(w.network(), w.ufvk(), p.diversifier_index).map_err(bad_request)?;
            w.address(p.diversifier_index).map_err(internal)?;
            derived
        };
        // A refund can only ever go to this address, so it must be one we can pay shielded.
        let refund_address = p.refund_address.as_deref().ok_or((StatusCode::UNPROCESSABLE_ENTITY, "proposal has no refund address".to_string()))?;
        self.wallet.lock().await.orchard_receiver(refund_address).map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, format!("refund address: {e}")))?;
        let price = self.prices.zec_usd().await;
        // Never approve different terms for an id the vault already has: that is equivocation.
        let existing = self.vault.order(&p.order_id).await.map_err(internal)?;
        let checked = check_proposal(&p, &derived, price, now_ns(), &self.limits).and_then(|()| check_against_vault(&p, existing.as_ref().map(|o| &o.terms)));
        if let Err(rejection) = checked {
            tracing::warn!(order = %p.order_id, %rejection, "rejected proposal");
            return Err((StatusCode::UNPROCESSABLE_ENTITY, rejection.to_string()));
        }

        // Record it before acting, so a crash between approving and recording can't lose it.
        {
            let db = self.db.lock().await;
            db.execute(
                "INSERT OR IGNORE INTO orders (id, proposal, created_ns) VALUES (?1, ?2, ?3)",
                params![p.order_id, serde_json::to_string(&p).map_err(internal)?, now_ns() as i64],
            )
            .map_err(internal)?;
        }
        let outcome = self.vault.approve_order(&self.signer, &p.order_id, &p.terms).await.map_err(internal)?;
        if let Some(f) = outcome.failure {
            return Err((StatusCode::CONFLICT, f));
        }
        self.record_signature(&p.order_id, "approved_tx", &outcome.tx_hash).await;
        tracing::info!(order = %p.order_id, tx = %outcome.tx_hash, "approved order");
        Ok(json!({ "approved": true, "tx": outcome.tx_hash, "state": outcome.value }))
    }

    async fn record_signature(&self, order: &str, column: &str, tx: &str) {
        let db = self.db.lock().await;
        let sql = format!("UPDATE orders SET {column} = ?1 WHERE id = ?2");
        if let Err(e) = db.execute(&sql, params![tx, order]) {
            tracing::error!(error = %e, "recording a signature");
        }
        self.status.lock().await.last_signature_at = Some(now_ns());
    }

    /// Looks at one order: if it is open on the vault and our own wallet shows it paid,
    /// approves the release. Returns what we decided.
    async fn evaluate(&self, id: &str) -> Result<Value> {
        let (proposal, release_tx): (Proposal, Option<String>) = {
            let db = self.db.lock().await;
            let row: Option<(String, Option<String>)> = db
                .query_row("SELECT proposal, release_tx FROM orders WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?;
            let (p, tx) = row.ok_or_else(|| anyhow::anyhow!("unknown order {id}"))?;
            (serde_json::from_str(&p)?, tx)
        };
        let Some(order) = self.vault.order(id).await? else {
            return Ok(json!({ "order": id, "decision": "not on chain" }));
        };
        if order.state != "open" {
            if matches!(order.state.as_str(), "released" | "refunded" | "expired" | "compensated") {
                let db = self.db.lock().await;
                db.execute("UPDATE orders SET final_state = ?1 WHERE id = ?2", params![order.state, id])?;
            }
            return Ok(json!({ "order": id, "decision": "nothing to do", "state": order.state }));
        }
        if release_tx.is_some() || order.release_approvals.contains(&self.signer.account_id) {
            return Ok(json!({ "order": id, "decision": "already approved release" }));
        }
        if let Some(decision) = self.settle_refund(id, &order.refund_approvals).await? {
            return Ok(decision);
        }

        let status = self.deposit_status(&proposal).await?;
        match &status {
            DepositStatus::Paid { .. } => {
                let outcome = self.vault.approve_release(&self.signer, id).await?;
                if let Some(f) = outcome.failure {
                    anyhow::bail!("approve_release failed: {f}");
                }
                self.record_signature(id, "release_tx", &outcome.tx_hash).await;
                tracing::info!(order = id, tx = %outcome.tx_hash, "approved release");
                Ok(json!({ "order": id, "decision": "approved release", "tx": outcome.tx_hash, "deposit": status }))
            }
            DepositStatus::Underpaid { .. } | DepositStatus::Late { .. } => {
                // Never release. The coordinator runs a FROST round for the refund.
                tracing::info!(order = id, ?status, "deposit needs a refund");
                Ok(json!({ "order": id, "decision": "needs refund", "deposit": status }))
            }
            _ => Ok(json!({ "order": id, "decision": "waiting for deposit", "deposit": status })),
        }
    }

    /// If this operator signed a refund for the order, approves it on the vault once its own
    /// wallet sees the refund mined at the required depth.
    async fn settle_refund(&self, id: &str, approvals: &[String]) -> Result<Option<Value>> {
        let (refund_tx, approved): (Option<String>, Option<String>) = {
            let db = self.db.lock().await;
            db.query_row("SELECT refund_tx, refund_approval_tx FROM orders WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?
        };
        let Some(txid) = refund_tx else { return Ok(None) };
        if approved.is_some() || approvals.contains(&self.signer.account_id) {
            return Ok(Some(json!({ "order": id, "decision": "already approved refund" })));
        }
        let (mined, tip) = {
            let w = self.wallet.lock().await;
            (w.tx_mined_height(&txid)?, w.heights()?.1.unwrap_or(0))
        };
        let depth = mined.map_or(0, |h| tip.saturating_sub(h) + 1);
        if depth < self.confirmations {
            return Ok(Some(json!({ "order": id, "decision": "waiting for refund confirmations", "refundTx": txid, "depth": depth })));
        }
        let outcome = self.vault.approve_refund(&self.signer, id).await?;
        if let Some(f) = outcome.failure {
            anyhow::bail!("approve_refund failed: {f}");
        }
        self.record_signature(id, "refund_approval_tx", &outcome.tx_hash).await;
        tracing::info!(order = id, tx = %outcome.tx_hash, refund = %txid, "approved refund");
        Ok(Some(json!({ "order": id, "decision": "approved refund", "tx": outcome.tx_hash })))
    }

    /// One refund per order. A different refund may be signed only once the earlier one can
    /// no longer be mined (unmined and past its expiry height); otherwise both could land.
    async fn may_sign_refund(&self, order_id: &str, txid: &str) -> Result<(), String> {
        let prev: Option<(Option<String>, Option<u32>)> = {
            let db = self.db.lock().await;
            db.query_row("SELECT refund_tx, refund_expiry FROM orders WHERE id = ?1", [order_id], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(|e| e.to_string())?
        };
        let Some((Some(prev), expiry)) = prev else { return Ok(()) };
        if prev == txid {
            return Ok(());
        }
        let w = self.wallet.lock().await;
        let mined = w.tx_mined_height(&prev).map_err(|e| e.to_string())?;
        let tip = w.heights().map_err(|e| e.to_string())?.1.unwrap_or(0);
        match (mined, expiry) {
            (None, Some(e)) if e > 0 && tip > e => Ok(()),
            (Some(h), _) => Err(format!("refund {prev} for this order was already mined at height {h}")),
            _ => Err(format!("already signed refund {prev} for this order; it can still be mined")),
        }
    }

    /// Round 1 of a refund signing round. Every check uses this operator's own data: its
    /// stored proposal (refund address), its own wallet (what arrived, and when), and the
    /// vault (the order is still open). Only then does it commit nonces.
    async fn round1(&self, req: Round1Request) -> Result<Round1Response, (StatusCode, String)> {
        let refuse = |m: String| (StatusCode::UNPROCESSABLE_ENTITY, m);
        let share = self.frost.as_ref().ok_or((StatusCode::SERVICE_UNAVAILABLE, "this operator has no FROST share configured".to_string()))?;
        let stored: Option<String> = {
            let db = self.db.lock().await;
            db.query_row("SELECT proposal FROM orders WHERE id = ?1", [&req.order_id], |r| r.get(0)).optional().map_err(internal)?
        };
        let proposal = stored.ok_or_else(|| refuse(format!("unknown order {}", req.order_id)))?;
        let proposal: Proposal = serde_json::from_str(&proposal).map_err(internal)?;
        let refund_address = proposal.refund_address.clone().ok_or_else(|| refuse("order has no refund address".into()))?;
        let order = self.vault.order(&req.order_id).await.map_err(internal)?.ok_or_else(|| refuse("order is not on the vault".into()))?;
        if order.state != "open" {
            return Err(refuse(format!("order is {} on the vault, not open", order.state)));
        }
        let received = match self.deposit_status(&proposal).await.map_err(internal)? {
            DepositStatus::Late { received_zat } => received_zat,
            DepositStatus::Underpaid { received_zat } if now_ns() > proposal.terms.deposit_deadline => received_zat,
            other => return Err(refuse(format!("deposit does not call for a refund ({other:?})"))),
        };

        let pczt = base64::engine::general_purpose::STANDARD.decode(&req.pczt).map_err(bad_request)?;
        let request = ssl_sign::prepare(&pczt).map_err(bad_request)?;
        let txid = txid_from_sighash(&request.sighash).unwrap_or_default();
        self.may_sign_refund(&req.order_id, &txid).await.map_err(refuse)?;
        let checked = {
            let w = self.wallet.lock().await;
            let tip = w.heights().map_err(internal)?.0.unwrap_or(0);
            let expiry = request.effects.expiry_height;
            if expiry == 0 || expiry > tip + MAX_REFUND_EXPIRY_BLOCKS {
                return Err(refuse(format!("refund expiry height {expiry} is not within {MAX_REFUND_EXPIRY_BLOCKS} blocks of {tip}")));
            }
            let receiver = w.orchard_receiver(&refund_address).map_err(internal)?;
            check_refund(&refund_tx(&request.effects), &receiver, received, self.max_refund_fee_zat, |a| w.is_treasury(a))
        };
        let refunded = checked.map_err(|r| {
            tracing::warn!(order = %req.order_id, rejection = %r, "refused to sign refund");
            refuse(r.to_string())
        })?;

        let sighash = hex::decode(&request.sighash).map_err(internal)?;
        let mut spends = vec![];
        let mut commitments = vec![];
        for s in &request.spends {
            let alpha: [u8; 32] = hex::decode(&s.alpha).ok().and_then(|b| b.try_into().ok()).ok_or_else(|| bad_request("bad alpha"))?;
            let (nonces, c) = share.commit();
            commitments.push(SpendCommitment { pool: s.pool, index: s.index, commitments: c });
            spends.push((s.pool, s.index, alpha, nonces));
        }
        let mut rounds = self.rounds.lock().await;
        rounds.retain(|_, r| r.opened.elapsed() < ROUND_TTL);
        rounds.insert(
            req.round_id.clone(),
            OpenRound { order_id: req.order_id.clone(), sighash, expiry_height: request.effects.expiry_height, spends, opened: std::time::Instant::now() },
        );
        tracing::info!(order = %req.order_id, round = %req.round_id, refunded, "committed to refund round");
        Ok(Round1Response { identifier: share.identifier_hex(), sighash: request.sighash, commitments })
    }

    /// Round 2: signs each spend's package, checking its message is the sighash this
    /// operator computed and using the alpha it derived. Nonces are consumed either way.
    async fn round2(&self, req: Round2Request) -> Result<Round2Response, (StatusCode, String)> {
        let share = self.frost.as_ref().ok_or((StatusCode::SERVICE_UNAVAILABLE, "no FROST share".to_string()))?;
        let round = self.rounds.lock().await.remove(&req.round_id).ok_or_else(|| bad_request("unknown or expired round"))?;
        if round.opened.elapsed() >= ROUND_TTL {
            return Err(bad_request("round expired"));
        }
        let txid = txid_from_sighash(&hex::encode(&round.sighash)).unwrap_or_default();
        // Decide and record under one lock: once shares leave, this is the order's refund.
        let _signing = self.signing.lock().await;
        self.may_sign_refund(&round.order_id, &txid).await.map_err(|m| (StatusCode::UNPROCESSABLE_ENTITY, m))?;
        let mut shares = vec![];
        for (pool, index, alpha, nonces) in round.spends {
            let pkg = req
                .packages
                .iter()
                .find(|p| p.pool == pool && p.index == index)
                .ok_or_else(|| bad_request(format!("no signing package for {pool:?} action {index}")))?;
            let s = share.sign(&pkg.signing_package, nonces, &round.sighash, &alpha).map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?;
            shares.push(SpendShare { pool, index, share: s });
        }
        {
            let db = self.db.lock().await;
            db.execute("UPDATE orders SET refund_tx = ?1, refund_expiry = ?2 WHERE id = ?3", params![txid, round.expiry_height, round.order_id]).map_err(internal)?;
        }
        self.status.lock().await.last_signature_at = Some(now_ns());
        tracing::info!(order = %round.order_id, refund_tx = %txid, "signed refund");
        Ok(Round2Response { identifier: share.identifier_hex(), shares })
    }

    async fn deposit_status(&self, p: &Proposal) -> Result<DepositStatus> {
        let (deposits, tip) = {
            let w = self.wallet.lock().await;
            let deposits = w.deposits_to(&p.deposit_address)?;
            let (_, scanned) = w.heights()?;
            (deposits, scanned.unwrap_or(0))
        };
        // First time this operator saw each deposit mined decides whether it was on time.
        let db = self.db.lock().await;
        let mut seen = vec![];
        for d in deposits {
            let first_seen = if d.mined_height.is_some() {
                db.execute("INSERT OR IGNORE INTO deposits_seen (txid, first_seen_mined_ns) VALUES (?1, ?2)", params![d.txid, now_ns() as i64])?;
                Some(db.query_row("SELECT first_seen_mined_ns FROM deposits_seen WHERE txid = ?1", [&d.txid], |r| r.get::<_, i64>(0))? as u64)
            } else {
                None
            };
            seen.push(SeenDeposit { txid: d.txid, mined_height: d.mined_height, value_zat: d.value_zat, first_seen_mined_ns: first_seen });
        }
        Ok(deposit_status(&seen, p.terms.zec_amount_zat, tip, self.confirmations, p.terms.deposit_deadline))
    }

    /// One pass: sync the wallet, then evaluate every order not yet final.
    async fn tick(&self) {
        let synced = { self.wallet.lock().await.sync().await };
        {
            let mut s = self.status.lock().await;
            match &synced {
                Ok(()) => {
                    s.last_sync_ok = Some(now_ns());
                    s.last_error = None;
                }
                Err(e) => s.last_error = Some(format!("sync: {e}")),
            }
        }
        if synced.is_err() {
            return;
        }
        let pending: Vec<String> = {
            let db = self.db.lock().await;
            let mut stmt = match db.prepare("SELECT id FROM orders WHERE final_state IS NULL") {
                Ok(s) => s,
                Err(e) => return tracing::error!(error = %e, "listing orders"),
            };
            stmt.query_map([], |r| r.get(0)).map(|rows| rows.filter_map(Result::ok).collect()).unwrap_or_default()
        };
        for id in pending {
            if let Err(e) = self.evaluate(&id).await {
                tracing::warn!(order = %id, error = %e, "evaluating order");
                self.status.lock().await.last_error = Some(format!("{id}: {e}"));
            }
        }
    }

    async fn health(&self) -> Value {
        let (tip, scanned) = self.wallet.lock().await.heights().unwrap_or((None, None));
        let bond = self.bond_usd().await;
        // Counted from the database so the figure survives restarts.
        let signatures: i64 = {
            let db = self.db.lock().await;
            db.query_row("SELECT COUNT(approved_tx) + COUNT(release_tx) + COUNT(refund_tx) + COUNT(refund_approval_tx) FROM orders", [], |r| r.get(0)).unwrap_or(0)
        };
        let s = self.status.lock().await;
        let fresh = s.last_sync_ok.is_some_and(|t| now_ns().saturating_sub(t) < 120_000_000_000);
        json!({
            "status": if fresh && s.last_error.is_none() { "ok" } else { "degraded" },
            "version": VERSION,
            "nearAccount": self.signer.account_id,
            "bondUsd": bond,
            "signaturesTotal": signatures,
            "lastSignatureAt": s.last_signature_at.map(ns_to_rfc3339),
            "frost": self.frost.as_ref().map(|f| f.identifier_hex()),
            "walletTip": tip,
            "walletScanned": scanned,
            "lastError": s.last_error,
        })
    }

    /// This operator's bond, refreshed at most once a minute: `/health` is polled often and
    /// should not cost an RPC call each time.
    async fn bond_usd(&self) -> Option<f64> {
        let mut cached = self.bond_cache.lock().await;
        if let Some((v, at)) = *cached {
            if at.elapsed() < Duration::from_secs(60) {
                return v;
            }
        }
        let v = match self.vault.operator(&self.signer.account_id).await {
            Ok(op) => op.and_then(|b| b.bond.parse::<f64>().ok()).map(|v| v / 10f64.powi(self.limits.token_decimals as i32)),
            Err(_) => return cached.and_then(|(v, _)| v),
        };
        *cached = Some((v, std::time::Instant::now()));
        v
    }

    fn authorized(&self, headers: &HeaderMap) -> bool {
        headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .is_some_and(|t| constant_time_eq(t.as_bytes(), self.token.as_bytes()))
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn internal(e: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn bad_request(e: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, e.to_string())
}

fn err_json((code, msg): (StatusCode, String)) -> Response {
    (code, Json(json!({ "approved": false, "error": msg }))).into_response()
}

type App = Arc<Node>;

async fn round1(State(n): State<App>, headers: HeaderMap, Json(r): Json<Round1Request>) -> Response {
    if !n.authorized(&headers) {
        return err_json((StatusCode::UNAUTHORIZED, "missing or wrong bearer token".into()));
    }
    // Check against the freshest view of the treasury.
    if let Err(e) = n.wallet.lock().await.sync().await {
        tracing::warn!(error = %e, "sync before round 1");
    }
    match n.round1(r).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err_json(e),
    }
}

async fn round2(State(n): State<App>, headers: HeaderMap, Json(r): Json<Round2Request>) -> Response {
    if !n.authorized(&headers) {
        return err_json((StatusCode::UNAUTHORIZED, "missing or wrong bearer token".into()));
    }
    match n.round2(r).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err_json(e),
    }
}

async fn health(State(n): State<App>) -> Json<Value> {
    Json(n.health().await)
}

async fn proposals(State(n): State<App>, headers: HeaderMap, Json(p): Json<Proposal>) -> Response {
    if !n.authorized(&headers) {
        return err_json((StatusCode::UNAUTHORIZED, "missing or wrong bearer token".into()));
    }
    match n.handle_proposal(p).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err_json(e),
    }
}

#[derive(Deserialize)]
struct ReleaseRequest {
    order_id: String,
}

async fn releases(State(n): State<App>, headers: HeaderMap, Json(r): Json<ReleaseRequest>) -> Response {
    if !n.authorized(&headers) {
        return err_json((StatusCode::UNAUTHORIZED, "missing or wrong bearer token".into()));
    }
    // Sync first so the decision uses the freshest view of the chain.
    let _ = n.wallet.lock().await.sync().await;
    match n.evaluate(&r.order_id).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err_json(internal(e)),
    }
}

async fn orders(State(n): State<App>) -> Response {
    let db = n.db.lock().await;
    let rows: Result<Vec<Value>> = (|| {
        let mut stmt = db.prepare("SELECT id, approved_tx, release_tx, final_state FROM orders ORDER BY created_ns DESC LIMIT 100")?;
        let rows = stmt.query_map([], |r| {
            Ok(json!({ "id": r.get::<_, String>(0)?, "approvedTx": r.get::<_, Option<String>>(1)?, "releaseTx": r.get::<_, Option<String>>(2)?, "finalState": r.get::<_, Option<String>>(3)? }))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    })();
    match rows {
        Ok(v) => Json(json!(v)).into_response(),
        Err(e) => err_json(internal(e)),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    ssl_node::init_tracing();
    ssl_wallet::install_crypto_provider();

    let listen: SocketAddr = config::listen_addr("OPERATOR_LISTEN", 7101)?;
    let token = config::req("OPERATOR_API_TOKEN")?;
    anyhow::ensure!(token.len() >= 24, "OPERATOR_API_TOKEN must be at least 24 characters");
    let vault = Vault::new(&config::opt("NEAR_RPC_URL", "https://rpc.testnet.fastnear.com"), &config::req("NEAR_VAULT_CONTRACT")?);
    let signer = match (std::env::var("NEAR_ACCOUNT_ID").ok().filter(|v| !v.is_empty()), std::env::var("NEAR_SECRET_KEY").ok().filter(|v| !v.is_empty())) {
        (Some(account), Some(key)) => Signer::new(account.trim(), key.trim())?,
        _ => Signer::from_credentials_file(config::req("NEAR_CREDENTIALS")?.as_ref())?,
    };
    let vault_config = vault.config().await.context("reading the vault config")?;
    let me = vault.operator(&signer.account_id).await?.with_context(|| format!("{} is not an operator on {}", signer.account_id, vault.contract))?;
    anyhow::ensure!(me.active, "{} has been removed as an operator", signer.account_id);

    let network = parse_network(&config::opt("ZCASH_NETWORK", "test"))?;
    let state_dir = PathBuf::from(config::opt("STATE_DIR", "./operator-state"));
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
    let max_usdc: u128 = config::num("MAX_ORDER_USDC", 1_000u128)?;
    let min_zec: f64 = config::num("MIN_ORDER_ZEC", 0.001)?;
    let limits = Limits {
        spread_bps: config::num("SPREAD_BPS", 25)?,
        price_tolerance_bps: config::num("PRICE_TOLERANCE_BPS", 200)?,
        token_decimals: decimals,
        max_amount_out: max_usdc * 10u128.pow(decimals as u32),
        min_zec_zat: (min_zec * 1e8) as u64,
        min_deposit_window_ns: 10 * 60 * 1_000_000_000,
        max_deposit_window_ns: 3 * 3600 * 1_000_000_000,
        min_diversifier_index: 1_000,
    };

    let frost = match (std::env::var("FROST_KEY_PACKAGE").ok().filter(|v| !v.is_empty()), std::env::var("FROST_CONFIG").ok().filter(|v| !v.is_empty())) {
        (Some(kp), _) => Some(Share::from_hex(&kp, &config::req("FROST_PUBLIC_KEY_PACKAGE")?)?),
        (None, Some(path)) => Some(Share::from_frost_client_config(path.as_ref(), &config::req("FROST_GROUP_KEY")?)?),
        (None, None) => None,
    };
    match &frost {
        Some(f) => tracing::info!(identifier = %f.identifier_hex(), group = %f.group_key_hex(), "FROST share loaded"),
        None => tracing::warn!("no FROST share configured; this operator cannot sign refunds"),
    }

    let node = Arc::new(Node {
        signer,
        vault,
        wallet: Mutex::new(wallet),
        db: Mutex::new(open_db(&state_dir.join("operator.sqlite"))?),
        prices: PriceFeed::coingecko(),
        limits,
        confirmations: config::num("CONFIRMATIONS", 12)?,
        token,
        status: Mutex::new(Status::default()),
        bond_cache: Mutex::new(None),
        frost,
        rounds: Mutex::new(HashMap::new()),
        signing: Mutex::new(()),
        max_refund_fee_zat: config::num("MAX_REFUND_FEE_ZAT", 20_000)?,
    });
    tracing::info!(account = %node.signer.account_id, vault = %node.vault.contract, %listen, "operator starting");

    let interval = Duration::from_secs(config::num("SYNC_INTERVAL_SECS", 20)?);
    let ticker = node.clone();
    tokio::spawn(async move {
        loop {
            ticker.tick().await;
            tokio::time::sleep(interval).await;
        }
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/proposals", post(proposals))
        .route("/v1/releases", post(releases))
        .route("/v1/signing/round1", post(round1))
        .route("/v1/signing/round2", post(round2))
        .route("/v1/orders", get(orders))
        .with_state(node);
    let listener = tokio::net::TcpListener::bind(listen).await?;
    axum::serve(listener, app).with_graceful_shutdown(async { let _ = tokio::signal::ctrl_c().await; }).await?;
    Ok(())
}
