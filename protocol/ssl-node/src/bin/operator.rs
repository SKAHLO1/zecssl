//! ZEC SSL operator node.
//!
//! An operator holds its own NEAR key and runs its own view-only treasury wallet. It:
//! - checks each order the coordinator proposes against its own rules, price feed and
//!   address derivation, and approves it on the NEAR vault only if every check passes;
//! - watches its own wallet for each approved order's deposit and approves the release
//!   once the deposit is paid in full, confirmed, and on time;
//! - reports its health at `GET /health`.
//!
//! It never trusts the coordinator: a proposal is just a request to check.
//!
//! Configuration (environment):
//!   OPERATOR_LISTEN          127.0.0.1:7101
//!   OPERATOR_API_TOKEN       bearer token the coordinator must present (required)
//!   NEAR_RPC_URL             https://rpc.testnet.fastnear.com
//!   NEAR_VAULT_CONTRACT      the vault account (required)
//!   NEAR_CREDENTIALS         path to this operator's NEAR CLI credentials file (required)
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
use ssl_node::{config, now_ns, ns_to_rfc3339, price::PriceFeed, vault::Vault, VERSION};
use ssl_policy::{check_proposal, deposit_status, DepositStatus, Limits, Proposal, SeenDeposit};
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
}

#[derive(Default)]
struct Status {
    last_sync_ok: Option<u64>,
    last_error: Option<String>,
    signatures: u64,
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
        let price = self.prices.zec_usd().await;
        if let Err(rejection) = check_proposal(&p, &derived, price, now_ns(), &self.limits) {
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
        let mut s = self.status.lock().await;
        s.signatures += 1;
        s.last_signature_at = Some(now_ns());
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
                // Refunds are signed with FROST on Zcash (Phase 6); never release.
                tracing::warn!(order = id, ?status, "deposit needs a refund");
                Ok(json!({ "order": id, "decision": "needs refund", "deposit": status }))
            }
            _ => Ok(json!({ "order": id, "decision": "waiting for deposit", "deposit": status })),
        }
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
        let bond = self.vault.operator(&self.signer.account_id).await.ok().flatten();
        let s = self.status.lock().await;
        let fresh = s.last_sync_ok.is_some_and(|t| now_ns().saturating_sub(t) < 120_000_000_000);
        json!({
            "status": if fresh && s.last_error.is_none() { "ok" } else { "degraded" },
            "version": VERSION,
            "nearAccount": self.signer.account_id,
            "bondUsd": bond.as_ref().and_then(|b| b.bond.parse::<f64>().ok()).map(|v| v / 10f64.powi(self.limits.token_decimals as i32)),
            "signaturesTotal": s.signatures,
            "lastSignatureAt": s.last_signature_at.map(ns_to_rfc3339),
            "walletTip": tip,
            "walletScanned": scanned,
            "lastError": s.last_error,
        })
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

    let listen: SocketAddr = config::opt("OPERATOR_LISTEN", "127.0.0.1:7101").parse()?;
    let token = config::req("OPERATOR_API_TOKEN")?;
    anyhow::ensure!(token.len() >= 24, "OPERATOR_API_TOKEN must be at least 24 characters");
    let vault = Vault::new(&config::opt("NEAR_RPC_URL", "https://rpc.testnet.fastnear.com"), &config::req("NEAR_VAULT_CONTRACT")?);
    let signer = Signer::from_credentials_file(config::req("NEAR_CREDENTIALS")?.as_ref())?;
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
        .route("/v1/orders", get(orders))
        .with_state(node);
    let listener = tokio::net::TcpListener::bind(listen).await?;
    axum::serve(listener, app).with_graceful_shutdown(async { let _ = tokio::signal::ctrl_c().await; }).await?;
    Ok(())
}
