use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use ssl_sign::{SigningRequest, SpendSignature};

#[derive(Parser)]
#[command(about = "Prepare a PCZT for FROST signing, and apply the signatures")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print the signing request (sighash, spends with their randomizers, effects) as JSON.
    Prepare {
        /// PCZT file, or "-" for stdin.
        #[arg(default_value = "-")]
        pczt: String,
        /// Also write each spend's message and randomizer as raw files for frost-client:
        /// <dir>/sighash.bin and <dir>/<pool>-<index>.alpha.bin
        #[arg(long)]
        out_dir: Option<PathBuf>,
        /// Refuse transactions with any transparent input or output.
        #[arg(long)]
        require_shielded: bool,
    },
    /// Apply signatures (a JSON array of {pool, index, signature}) and write the signed PCZT.
    Apply {
        #[arg(long)]
        pczt: PathBuf,
        #[arg(long)]
        signatures: PathBuf,
        /// Output file, or "-" for stdout.
        #[arg(long, default_value = "-")]
        out: String,
    },
    /// Extract the finished transaction (verifying proofs and signatures) and report it
    /// as JSON: txid, transparent input/output counts, raw hex.
    Extract {
        #[arg(default_value = "-")]
        pczt: String,
    },
    /// Verify a signature against the group key rerandomized by alpha. All values hex.
    Verify {
        #[arg(long)]
        ak: String,
        #[arg(long)]
        alpha: String,
        #[arg(long)]
        sighash: String,
        #[arg(long)]
        signature: String,
    },
}

fn read_input(path: &str) -> Result<Vec<u8>> {
    if path == "-" {
        let mut buf = vec![];
        std::io::stdin().read_to_end(&mut buf)?;
        Ok(buf)
    } else {
        fs::read(path).with_context(|| format!("reading {path}"))
    }
}

fn main() -> Result<()> {
    match Args::parse().command {
        Command::Prepare { pczt, out_dir, require_shielded } => {
            let request: SigningRequest = ssl_sign::prepare(&read_input(&pczt)?)?;
            if require_shielded && !request.effects.is_fully_shielded() {
                bail!(
                    "refusing: {} transparent input(s) and {} transparent output(s)",
                    request.effects.transparent_inputs,
                    request.effects.transparent_outputs
                );
            }
            if let Some(dir) = out_dir {
                fs::create_dir_all(&dir)?;
                fs::write(dir.join("sighash.bin"), hex::decode(&request.sighash)?)?;
                for s in &request.spends {
                    let pool = serde_json::to_value(s.pool)?.as_str().unwrap_or_default().to_owned();
                    fs::write(dir.join(format!("{pool}-{}.alpha.bin", s.index)), hex::decode(&s.alpha)?)?;
                }
            }
            println!("{}", serde_json::to_string_pretty(&request)?);
        }
        Command::Apply { pczt, signatures, out } => {
            let sigs: Vec<SpendSignature> = serde_json::from_slice(&fs::read(&signatures)?)?;
            let signed = ssl_sign::apply(&fs::read(&pczt)?, &sigs)?;
            if out == "-" {
                std::io::stdout().write_all(&signed)?;
            } else {
                fs::write(&out, signed)?;
            }
        }
        Command::Extract { pczt } => {
            let tx = ssl_sign::extract(&read_input(&pczt)?)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "txid": tx.txid,
                    "transparent_inputs": tx.transparent_inputs,
                    "transparent_outputs": tx.transparent_outputs,
                    "fully_shielded": tx.transparent_inputs == 0 && tx.transparent_outputs == 0,
                    "raw": hex::encode(&tx.raw),
                }))?
            );
        }
        Command::Verify { ak, alpha, sighash, signature } => {
            ssl_sign::verify(&ak, &alpha, &sighash, &signature)?;
            println!("valid");
        }
    }
    Ok(())
}
