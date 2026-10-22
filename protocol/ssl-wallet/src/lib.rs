//! The ZEC SSL treasury as seen through its viewing key.
//!
//! Both the coordinator and every operator run one of these, each synced from its own
//! lightwalletd, so each party detects deposits independently. It can derive the
//! per-order deposit address for any diversifier index, list what arrived at an address,
//! and report the treasury's shielded balance. It holds no spending key.

mod cache;

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use rand::{rand_core::UnwrapErr, rngs::SysRng};
use tonic::transport::{Channel, ClientTlsConfig};
use zcash_client_backend::data_api::{Account as _, AccountBirthday, AccountPurpose, WalletRead, WalletWrite};
use zcash_client_backend::proto::service::{self, compact_tx_streamer_client::CompactTxStreamerClient};
use zcash_client_backend::sync;
use zcash_client_sqlite::util::SystemClock;
use zcash_client_sqlite::wallet::init::init_wallet_db;
use zcash_client_sqlite::{AccountUuid, WalletDb};
use zcash_keys::keys::{UnifiedAddressRequest, UnifiedFullViewingKey};
use zcash_protocol::consensus::{BlockHeight, Network};
use zip32::DiversifierIndex;

pub use cache::MemBlockCache;

type Db = WalletDb<rusqlite::Connection, Network, SystemClock, UnwrapErr<SysRng>>;

/// Something that arrived at a treasury address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Deposit {
    /// Display order (as explorers show it).
    pub txid: String,
    /// `None` while unmined.
    pub mined_height: Option<u32>,
    pub value_zat: u64,
    /// The memo as text, if it decodes as UTF-8 text.
    pub memo: Option<String>,
    /// 3 = Orchard, 4 = Ironwood.
    pub pool: u8,
}

pub fn parse_network(s: &str) -> Result<Network> {
    match s {
        "main" | "mainnet" => Ok(Network::MainNetwork),
        "test" | "testnet" => Ok(Network::TestNetwork),
        other => bail!("unknown network {other:?} (use main or test)"),
    }
}

pub fn parse_ufvk(network: &Network, s: &str) -> Result<UnifiedFullViewingKey> {
    UnifiedFullViewingKey::decode(network, s.trim()).map_err(|e| anyhow!("invalid unified full viewing key: {e}"))
}

/// The shielded deposit address for an order, derived from the viewing key alone. Pure,
/// so an operator can check a coordinator-supplied address without trusting it.
pub fn derive_address(network: &Network, ufvk: &UnifiedFullViewingKey, index: u64) -> Result<String> {
    let ua = ufvk
        .address(DiversifierIndex::from(index), UnifiedAddressRequest::SHIELDED)
        .map_err(|e| anyhow!("no shielded address at diversifier index {index}: {e:?}"))?;
    Ok(ua.encode(network))
}

pub struct TreasuryWallet {
    network: Network,
    ufvk: UnifiedFullViewingKey,
    db: Db,
    db_path: PathBuf,
    account: AccountUuid,
    lightwalletd: String,
    cache: MemBlockCache,
}

/// Selects rustls' crypto backend for the process. Needed because more than one is compiled
/// in; safe to call repeatedly.
pub fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

async fn connect(url: &str) -> Result<CompactTxStreamerClient<Channel>> {
    install_crypto_provider();
    let uri: tonic::transport::Uri = url.parse().with_context(|| format!("invalid lightwalletd URL {url}"))?;
    let mut endpoint = Channel::from_shared(url.to_string())?;
    if uri.scheme_str() == Some("https") {
        let host = uri.host().ok_or_else(|| anyhow!("lightwalletd URL has no host"))?.to_string();
        endpoint = endpoint.tls_config(ClientTlsConfig::new().domain_name(host).assume_http2(true).with_webpki_roots())?;
    }
    Ok(CompactTxStreamerClient::new(endpoint.connect().await.with_context(|| format!("connecting to {url}"))?))
}

impl TreasuryWallet {
    /// Opens (or creates) the wallet in `dir`. A new wallet imports the viewing key with a
    /// birthday of `birthday`, or 100 blocks before the current tip.
    pub async fn open(dir: &Path, network: Network, ufvk: &str, lightwalletd: &str, birthday: Option<u32>) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let ufvk = parse_ufvk(&network, ufvk)?;
        let db_path = dir.join("treasury.sqlite");
        let mut db = WalletDb::for_path(&db_path, network, SystemClock, UnwrapErr(SysRng))?;
        init_wallet_db(&mut db, None).map_err(|e| anyhow!("initializing the wallet database: {e:?}"))?;

        let account = match db.get_account_for_ufvk(&ufvk)? {
            Some(a) => a.id(),
            None => {
                let mut client = connect(lightwalletd).await?;
                let tip: u32 = client.get_latest_block(service::ChainSpec::default()).await?.into_inner().height.try_into()?;
                let height = birthday.unwrap_or(tip.saturating_sub(100));
                let treestate = client
                    .get_tree_state(service::BlockId { height: u64::from(height.saturating_sub(1)), ..Default::default() })
                    .await?
                    .into_inner();
                let birthday = AccountBirthday::from_treestate(treestate, Some(BlockHeight::from_u32(tip)))
                    .map_err(|e| anyhow!("building the wallet birthday: {e:?}"))?;
                tracing::info!(height, "importing the treasury viewing key");
                db.import_account_ufvk("treasury", &ufvk, &birthday, AccountPurpose::ViewOnly, None)?.id()
            }
        };
        Ok(Self { network, ufvk, db, db_path, account, lightwalletd: lightwalletd.to_string(), cache: MemBlockCache::default() })
    }

    pub fn network(&self) -> &Network {
        &self.network
    }

    pub fn is_testnet(&self) -> bool {
        matches!(self.network, Network::TestNetwork)
    }

    pub fn ufvk(&self) -> &UnifiedFullViewingKey {
        &self.ufvk
    }

    /// The deposit address for `index`, recorded in the wallet so that notes received at it
    /// are attributed to it.
    pub fn address(&mut self, index: u64) -> Result<String> {
        let ua = self
            .db
            .get_address_for_index(self.account, DiversifierIndex::from(index), UnifiedAddressRequest::SHIELDED)?
            .ok_or_else(|| anyhow!("no shielded address at diversifier index {index}"))?;
        Ok(ua.encode(&self.network))
    }

    /// Scans the chain up to the current tip.
    pub async fn sync(&mut self) -> Result<()> {
        let mut client = connect(&self.lightwalletd).await?;
        sync::run(&mut client, &self.network, &self.cache, &mut self.db, 1_000)
            .await
            .map_err(|e| anyhow!("sync failed: {e}"))
    }

    /// The chain tip the wallet knows about, and the highest block it has fully scanned.
    pub fn heights(&self) -> Result<(Option<u32>, Option<u32>)> {
        let tip = self.db.chain_height()?.map(u32::from);
        let conn = self.read_conn()?;
        let scanned: Option<u32> = conn.query_row("SELECT MAX(height) FROM blocks", [], |r| r.get(0))?;
        Ok((tip, scanned))
    }

    fn read_conn(&self) -> Result<rusqlite::Connection> {
        Ok(rusqlite::Connection::open_with_flags(&self.db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?)
    }

    /// Everything received at `address`, oldest first. Change outputs are excluded.
    pub fn deposits_to(&self, address: &str) -> Result<Vec<Deposit>> {
        let conn = self.read_conn()?;
        let mut stmt = conn.prepare(
            // Rows with a receiving account are outputs this wallet received.
            "SELECT txid, tx_mined_height, value, memo, output_pool
             FROM v_tx_outputs
             WHERE to_account_uuid IS NOT NULL AND is_change = 0 AND to_address = ?1
             ORDER BY tx_mined_height IS NULL, tx_mined_height",
        )?;
        let rows = stmt.query_map([address], |r| {
            let txid: Vec<u8> = r.get(0)?;
            let memo: Option<Vec<u8>> = r.get(3)?;
            Ok(Deposit {
                txid: txid.iter().rev().map(|b| format!("{b:02x}")).collect(),
                mined_height: r.get(1)?,
                value_zat: r.get::<_, i64>(2)? as u64,
                memo: memo.and_then(memo_text),
                pool: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Unspent shielded value held by the treasury, in zatoshis (unconfirmed included).
    pub fn balance_zat(&self) -> Result<u64> {
        let conn = self.read_conn()?;
        let v: Option<i64> = conn.query_row(
            "SELECT SUM(ro.value) FROM v_received_outputs ro
             WHERE ro.pool IN (3, 4)
               AND NOT EXISTS (SELECT 1 FROM v_received_output_spends s
                               WHERE s.pool = ro.pool AND s.received_output_id = ro.id_within_pool_table)",
            [],
            |r| r.get(0),
        )?;
        Ok(v.unwrap_or(0) as u64)
    }
}

/// ZIP 302: a memo whose first byte is <= 0xF4 is UTF-8 text, zero-padded.
fn memo_text(bytes: Vec<u8>) -> Option<String> {
    if bytes.first().is_none_or(|b| *b > 0xF4) {
        return None;
    }
    let end = bytes.iter().rposition(|b| *b != 0).map_or(0, |i| i + 1);
    String::from_utf8(bytes[..end].to_vec()).ok().filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memo_text_strips_zero_padding_and_rejects_binary_memos() {
        let mut memo = b"zs-17".to_vec();
        memo.resize(512, 0);
        assert_eq!(memo_text(memo).as_deref(), Some("zs-17"));
        assert_eq!(memo_text(vec![0xF6; 512]), None);
        assert_eq!(memo_text(vec![0; 512]), None);
    }
}
