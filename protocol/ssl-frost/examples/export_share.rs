//! Prints an operator's FROST share as the environment variables a hosted operator reads
//! (FROST_KEY_PACKAGE is secret: paste it into the host's secret store, nowhere else).
//!
//!   cargo run -p ssl-frost --example export_share -- ~/zec-ssl-spike/operators/op1.toml "$(cat ~/zec-ssl-spike/group.key)"

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    anyhow::ensure!(args.len() == 3, "usage: export_share <frost-client config> <group key hex>");
    let share = ssl_frost::Share::from_frost_client_config(args[1].as_ref(), &args[2])?;
    let (key_package, public) = share.to_hex()?;
    println!("FROST_KEY_PACKAGE={key_package}");
    println!("FROST_PUBLIC_KEY_PACKAGE={public}");
    Ok(())
}
