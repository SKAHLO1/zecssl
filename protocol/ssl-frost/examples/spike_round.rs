//! Runs a native 2-of-3 round with the Phase 2 DKG shares (protocol/spike) over a real v6
//! sighash and randomizer, and verifies the result independently with ssl-sign.
//!   cargo run -p ssl-frost --example spike_round -- <spike-dir> <signer-a> <signer-b> <pczt>

use std::collections::BTreeMap;
use std::path::PathBuf;

use ssl_frost::{Coordinator, Share};

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    anyhow::ensure!(a.len() == 4, "usage: spike_round <spike-dir> <signer-a> <signer-b> <pczt>");
    let dir = PathBuf::from(&a[0]);
    let group = std::fs::read_to_string(dir.join("group.key"))?.trim().to_string();
    let cfg = |op: &str| dir.join("operators").join(format!("{op}.toml"));

    let signers = [Share::from_frost_client_config(&cfg(&a[1]), &group)?, Share::from_frost_client_config(&cfg(&a[2]), &group)?];
    // The coordinator reads only the public key package, from the non-signer's config.
    let coord_op = ["op1", "op2", "op3"].into_iter().find(|o| *o != a[1] && *o != a[2]).unwrap();
    let coordinator = Coordinator::from_frost_client_config(&cfg(coord_op), &group)?;
    anyhow::ensure!(coordinator.group_key_hex() == group, "public package does not match the group key");

    let request = ssl_sign::prepare(&std::fs::read(&a[3])?)?;
    let sighash = hex::decode(&request.sighash)?;
    let spend = &request.spends[0];
    let alpha: [u8; 32] = hex::decode(&spend.alpha)?.try_into().map_err(|_| anyhow::anyhow!("alpha length"))?;

    let mut nonces = vec![];
    let mut commitments = BTreeMap::new();
    for s in &signers {
        let (n, c) = s.commit();
        nonces.push(n);
        commitments.insert(s.identifier_hex(), c);
    }
    let package = coordinator.signing_package(&commitments, &sighash)?;
    let mut shares = BTreeMap::new();
    for (s, n) in signers.iter().zip(nonces) {
        shares.insert(s.identifier_hex(), s.sign(&package, n, &sighash, &alpha)?);
    }
    let sig = coordinator.aggregate(&package, &shares, &alpha)?;
    ssl_sign::verify(&group, &spend.alpha, &request.sighash, &sig)?;
    println!("{} + {} signed natively; {} coordinated with public data only; signature verifies: {}…", a[1], a[2], coord_op, &sig[..16]);
    Ok(())
}
