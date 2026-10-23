//! Failure-path test: plays a malicious coordinator. Builds a real, proven refund for an
//! order that is due one, but wrong in a chosen way, and asks an operator to sign it in
//! round 1. The operator must refuse every variant.
//!
//!   bad_refund <wallet dir> <ufvk file> <operator url> <token> <order id> <received zat> <refund address> <stranger address>
//!
//! The wallet dir should be a copy of a synced treasury wallet: building a PCZT writes to it.

use std::path::Path;

use anyhow::Result;
use base64::Engine as _;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<()> {
    ssl_wallet::install_crypto_provider();
    let a: Vec<String> = std::env::args().collect();
    anyhow::ensure!(a.len() == 9, "usage: bad_refund <wallet dir> <ufvk file> <operator url> <token> <order id> <received zat> <refund address> <stranger address>");
    let (dir, ufvk, url, token, order, received, refund, stranger) = (&a[1], &a[2], &a[3], &a[4], &a[5], a[6].parse::<u64>()?, &a[7], &a[8]);
    let mut wallet = ssl_wallet::TreasuryWallet::open(
        Path::new(dir),
        ssl_wallet::parse_network("test")?,
        std::fs::read_to_string(ufvk)?.trim(),
        "https://testnet.zec.rocks:443",
        None,
    )
    .await?;
    wallet.sync().await?;

    let variants: [(&str, &str, u64); 3] = [
        ("pays a stranger instead of the refund address", stranger, received - 10_000),
        ("skims: refunds half and keeps the rest in the treasury", refund, received / 2),
        ("overpays from the treasury", refund, received * 3),
    ];
    let http = reqwest::Client::new();
    let mut refused = 0;
    for (i, (what, to, value)) in variants.iter().enumerate() {
        let pczt = wallet.create_payment(to, *value, &format!("zec-ssl refund {order}"))?;
        let proved = ssl_wallet::prove(&pczt)?;
        let res = http
            .post(format!("{url}/v1/signing/round1"))
            .bearer_auth(token)
            .json(&json!({
                "round_id": format!("evil-{i}"),
                "kind": "refund",
                "order_id": order,
                "pczt": base64::engine::general_purpose::STANDARD.encode(&proved),
            }))
            .send()
            .await?;
        let status = res.status();
        let body = res.text().await?;
        let ok = status.is_client_error();
        refused += ok as usize;
        println!("{} {what}: HTTP {status} {body}", if ok { "REFUSED" } else { "SIGNED?!" });
    }
    anyhow::ensure!(refused == variants.len(), "an operator committed to a bad refund");
    println!("all {} bad refunds refused", variants.len());
    Ok(())
}
