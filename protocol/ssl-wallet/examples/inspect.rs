//! Syncs a treasury wallet and prints an address and what arrived at it.
//!   cargo run -p ssl-wallet --example inspect -- <dir> <ufvk-file> <lightwalletd-url> <birthday> <index>

use ssl_wallet::{derive_address, parse_network, TreasuryWallet};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    anyhow::ensure!(a.len() == 5, "usage: inspect <dir> <ufvk-file> <lightwalletd-url> <birthday> <index>");
    let network = parse_network("test")?;
    let ufvk = std::fs::read_to_string(&a[1])?;
    let mut w = TreasuryWallet::open(a[0].as_ref(), network, &ufvk, &a[2], Some(a[3].parse()?)).await?;
    let started = std::time::Instant::now();
    w.sync().await?;
    let index: u64 = a[4].parse()?;
    let addr = w.address(index)?;
    let pure = derive_address(&network, w.ufvk(), index)?;
    println!("synced in {:?}; heights {:?}", started.elapsed(), w.heights()?);
    println!("address[{index}] = {addr}");
    println!("pure derivation matches: {}", addr == pure);
    println!("balance: {} zat", w.balance_zat()?);
    for d in w.deposits_to(&addr)? {
        println!("  {} height {:?} {} zat pool {} memo {:?}", d.txid, d.mined_height, d.value_zat, d.pool, d.memo);
    }
    Ok(())
}
