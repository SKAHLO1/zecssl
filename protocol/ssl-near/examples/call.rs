//! Sends one function call. Usage:
//!   cargo run -p ssl-near --example call -- <rpc-url> <credentials.json> <contract> <method> '<json args>' [deposit-yocto]

use ssl_near::{Near, Signer, TGAS};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    anyhow::ensure!(a.len() == 5 || a.len() == 6, "usage: call <rpc-url> <credentials.json> <contract> <method> '<json args>' [deposit-yocto]");
    let deposit: u128 = a.get(5).map(|d| d.parse()).transpose()?.unwrap_or(0);
    let signer = Signer::from_credentials_file(a[1].as_ref())?;
    let outcome = Near::new(&a[0]).call(&signer, &a[2], &a[3], serde_json::from_str(&a[4])?, 30 * TGAS, deposit).await?;
    println!("tx {}", outcome.tx_hash);
    match outcome.failure {
        Some(f) => println!("executed, failed: {f}"),
        None => println!("executed, ok: {:?}", outcome.value),
    }
    Ok(())
}
