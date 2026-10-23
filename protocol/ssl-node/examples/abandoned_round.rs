//! Failure-path test: a coordinator that crashes between the two FROST rounds.
//!
//!   abandoned_round open <wallet dir> <ufvk file> <frost config> <group key> <token> <order> <received zat> <refund address> <state.json> <operator url>...
//!     builds a correct refund, runs round 1 with the operators, saves the signing packages
//!     it would have sent, and stops: the crash. Operators are left holding nonces.
//!   abandoned_round replay <token> <state.json>
//!     sends the abandoned round 2 later, after a fresh round has refunded the order. Every
//!     operator must refuse: the order's refund is already signed and can still be mined.
//!
//! The wallet dir should be a copy of a synced treasury wallet: building a PCZT writes to it.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use base64::Engine as _;
use serde_json::{json, Value};
use ssl_node::signing::{Round1Response, SpendPackage};

async fn post(http: &reqwest::Client, url: &str, token: &str, body: &Value) -> Result<(reqwest::StatusCode, String)> {
    let res = http.post(url).bearer_auth(token).json(body).send().await?;
    Ok((res.status(), res.text().await?))
}

#[tokio::main]
async fn main() -> Result<()> {
    ssl_wallet::install_crypto_provider();
    let a: Vec<String> = std::env::args().collect();
    let http = reqwest::Client::new();
    match a.get(1).map(String::as_str) {
        Some("open") if a.len() >= 12 => {
            let (dir, ufvk, frost_cfg, group, token, order, received, refund, state) =
                (&a[2], &a[3], &a[4], &a[5], &a[6], &a[7], a[8].parse::<u64>()?, &a[9], &a[10]);
            let ops = &a[11..];
            let frost = ssl_frost::Coordinator::from_frost_client_config(frost_cfg.as_ref(), group)?;
            let mut wallet = ssl_wallet::TreasuryWallet::open(
                Path::new(dir),
                ssl_wallet::parse_network("test")?,
                std::fs::read_to_string(ufvk)?.trim(),
                "https://testnet.zec.rocks:443",
                None,
            )
            .await?;
            wallet.sync().await?;
            let pczt = wallet.create_payment(refund, received - 10_000, &format!("zec-ssl refund {order}"))?;
            let proved = ssl_wallet::prove(&pczt)?;
            let request = ssl_sign::prepare(&proved)?;
            let round_id = format!("crashed-{order}");
            let body = json!({ "round_id": round_id, "kind": "refund", "order_id": order, "pczt": base64::engine::general_purpose::STANDARD.encode(&proved) });

            let mut responses = vec![];
            for op in ops {
                let (status, text) = post(&http, &format!("{op}/v1/signing/round1"), token, &body).await?;
                if !status.is_success() {
                    bail!("{op} refused round 1: {text}");
                }
                responses.push(serde_json::from_str::<Round1Response>(&text).context("round 1 reply")?);
                println!("{op} committed to {round_id}");
            }
            let message = hex::decode(&request.sighash)?;
            let mut packages = vec![];
            for spend in &request.spends {
                let mut commitments = BTreeMap::new();
                for r in &responses {
                    let c = r.commitments.iter().find(|c| c.pool == spend.pool && c.index == spend.index).context("missing commitment")?;
                    commitments.insert(r.identifier.clone(), c.commitments.clone());
                }
                packages.push(SpendPackage { pool: spend.pool, index: spend.index, signing_package: frost.signing_package(&commitments, &message)? });
            }
            std::fs::write(state, serde_json::to_vec_pretty(&json!({ "round_id": round_id, "operators": ops, "packages": packages, "txid": ssl_node::signing::txid_from_sighash(&request.sighash) }))?)?;
            println!("crashed before round 2; abandoned refund would have been {}", ssl_node::signing::txid_from_sighash(&request.sighash).unwrap_or_default());
        }
        Some("replay") if a.len() == 4 => {
            let (token, state) = (&a[2], &a[3]);
            let saved: Value = serde_json::from_slice(&std::fs::read(state)?)?;
            let body = json!({ "round_id": saved["round_id"], "packages": saved["packages"] });
            let mut refused = 0;
            let ops = saved["operators"].as_array().context("operators")?;
            for op in ops {
                let op = op.as_str().context("operator url")?;
                let (status, text) = post(&http, &format!("{op}/v1/signing/round2"), token, &body).await?;
                let ok = status.is_client_error();
                refused += ok as usize;
                println!("{} {op}: HTTP {status} {text}", if ok { "REFUSED" } else { "SIGNED?!" });
            }
            anyhow::ensure!(refused == ops.len(), "an operator signed the abandoned round");
            println!("abandoned round refused by all {} operators", ops.len());
        }
        _ => bail!("usage: see the comment at the top of examples/abandoned_round.rs"),
    }
    Ok(())
}
