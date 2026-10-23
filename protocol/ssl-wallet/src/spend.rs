//! Building treasury payments (refunds) for external signing: propose, create the PCZT,
//! prove it, and broadcast the signed result. The wallet holds only the viewing key, so
//! the PCZT leaves here unsigned; the operators' FROST round supplies the signatures.
#![allow(deprecated)]

use std::num::NonZeroUsize;
use std::str::FromStr;
use std::sync::OnceLock;

use anyhow::{anyhow, Result};
use pczt::roles::prover::Prover;
use pczt::Pczt;
use rand::{rand_core::UnwrapErr, rngs::SysRng};
use zcash_address::ZcashAddress;
use zcash_client_backend::data_api::wallet::input_selection::{GreedyInputSelector, SpendPolicy};
use zcash_client_backend::data_api::wallet::{create_pczt_from_proposal, propose_transfer, ConfirmationsPolicy};
use zcash_client_backend::fees::standard::MultiOutputChangeStrategy;
use zcash_client_backend::fees::{DustOutputPolicy, SplitPolicy, StandardFeeRule};
use zcash_client_backend::proto::service;
use zcash_client_backend::wallet::OvkPolicy;
use zcash_client_sqlite::util::SystemClock;
use zcash_keys::address::Address;
use zcash_protocol::consensus::BranchId;
use zcash_protocol::memo::{Memo, MemoBytes};
use zcash_protocol::value::Zatoshis;
use zcash_protocol::ShieldedPool;
use zip321::{Payment, TransactionRequest};

use crate::{connect, TreasuryWallet};

type WalletErrorT = zcash_client_backend::data_api::error::Error<
    zcash_client_sqlite::error::SqliteClientError,
    zcash_client_sqlite::wallet::commitment_tree::Error,
    zcash_client_backend::data_api::wallet::input_selection::GreedyInputSelectorError,
    zcash_primitives::transaction::fees::zip317::FeeError,
    zcash_primitives::transaction::fees::zip317::FeeError,
    zcash_client_sqlite::ReceivedNoteId,
>;

impl TreasuryWallet {
    /// The Orchard-protocol receiver of a unified address, as raw bytes. Refunds are
    /// checked against this, never against an address string.
    pub fn orchard_receiver(&self, address: &str) -> Result<[u8; 43]> {
        match Address::decode(&self.network, address) {
            Some(Address::Unified(ua)) => ua.orchard().map(|a| a.to_raw_address_bytes()).ok_or_else(|| anyhow!("address has no shielded Orchard-protocol receiver")),
            Some(_) => Err(anyhow!("refund addresses must be unified addresses")),
            None => Err(anyhow!("not a valid address for this network")),
        }
    }

    /// Whether a raw Orchard-protocol address belongs to the treasury (any scope, so change
    /// outputs count), checked by the viewing key.
    pub fn is_treasury(&self, raw: &[u8; 43]) -> bool {
        let Some(fvk) = self.ufvk.orchard() else { return false };
        let addr = orchard::Address::from_raw_address_bytes(raw);
        if addr.is_none().into() {
            return false;
        }
        fvk.scope_for_address(&addr.unwrap()).is_some()
    }

    /// An unsigned, unproven PCZT paying `value_zat` to `to` with `memo`, funded from the
    /// treasury with change back to it.
    pub fn create_payment(&mut self, to: &str, value_zat: u64, memo: &str) -> Result<Vec<u8>> {
        let change = MultiOutputChangeStrategy::new(
            StandardFeeRule::Zip317,
            None,
            ShieldedPool::Orchard,
            DustOutputPolicy::default(),
            SplitPolicy::with_min_output_value(NonZeroUsize::new(1).expect("1 is nonzero"), Zatoshis::from_u64(10_000_000)?),
        );
        let request = TransactionRequest::new(vec![Payment::new(
            ZcashAddress::from_str(to).map_err(|e| anyhow!("invalid recipient: {e}"))?,
            Some(Zatoshis::from_u64(value_zat)?),
            Some(MemoBytes::from(Memo::from_str(memo).map_err(|e| anyhow!("invalid memo: {e:?}"))?)),
            None,
            None,
            vec![],
        )
        .map_err(|e| anyhow!("payment: {e:?}"))?])
        .map_err(|e| anyhow!("request: {e:?}"))?;

        let proposal = propose_transfer::<_, _, _, _, zcash_client_sqlite::wallet::commitment_tree::Error>(
            &mut self.db,
            &self.network,
            self.account,
            &GreedyInputSelector::new(),
            &change,
            request,
            ConfirmationsPolicy::default(),
            &SpendPolicy::default(),
            None,
            None,
        )
        .map_err(|e| anyhow!("proposing the payment: {e}"))?;
        let pczt: Result<Pczt, WalletErrorT> = create_pczt_from_proposal(
            &mut self.db,
            &self.network,
            &SystemClock,
            &mut UnwrapErr(SysRng),
            self.account,
            OvkPolicy::Sender,
            &proposal,
            None,
            zcash_primitives::transaction::builder::BundlePadding::DEFAULT,
        )
        ;
        pczt.map_err(|e| anyhow!("creating the PCZT: {e}"))?.serialize().map_err(|e| anyhow!("serializing the PCZT: {e:?}"))
    }

    /// Sends a finished transaction through lightwalletd. The returned future does not
    /// borrow the wallet, so callers can release it before awaiting.
    pub fn broadcast(&self, raw_tx: &[u8]) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
        let (url, data) = (self.lightwalletd.clone(), raw_tx.to_vec());
        async move {
            let mut client = connect(&url).await?;
            let res = client.send_transaction(service::RawTransaction { data, height: 0 }).await?.into_inner();
            if res.error_code != 0 {
                return Err(anyhow!("lightwalletd rejected the transaction ({}): {}", res.error_code, res.error_message));
            }
            Ok(())
        }
    }

    /// The height a transaction was mined at, if this wallet has seen it mined.
    pub fn tx_mined_height(&self, txid_display_hex: &str) -> Result<Option<u32>> {
        let mut txid = hex::decode(txid_display_hex)?;
        txid.reverse();
        let conn = self.read_conn()?;
        let h: Option<Option<u32>> = conn
            .query_row("SELECT mined_height FROM transactions WHERE txid = ?1", [txid], |r| r.get(0))
            .map(Some)
            .or_else(|e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(e) })?;
        Ok(h.flatten())
    }
}

/// Adds the Halo 2 proofs. Needs no key and no wallet, so callers can run it without
/// holding the wallet.
pub fn prove(pczt_bytes: &[u8]) -> Result<Vec<u8>> {
    let pczt = Pczt::parse(pczt_bytes).map_err(|e| anyhow!("invalid PCZT: {e:?}"))?;
    if !pczt.sapling().spends().is_empty() || !pczt.sapling().outputs().is_empty() {
        return Err(anyhow!("treasury payments never touch Sapling"));
    }
    let branch = BranchId::try_from(*pczt.global().consensus_branch_id()).map_err(|_| anyhow!("unknown consensus branch"))?;
    let version = zcash_primitives::transaction::components::orchard::bundle_version_for_branch(branch, orchard::ValuePool::Orchard)
        .ok_or_else(|| anyhow!("branch does not support Orchard-protocol bundles"))?
        .circuit_version();
    // Building the proving key takes seconds; build it once per process.
    static PK: OnceLock<orchard::circuit::ProvingKey> = OnceLock::new();
    let pk = PK.get_or_init(|| orchard::circuit::ProvingKey::build(version));

    let mut prover = Prover::new(pczt);
    if prover.requires_orchard_proof() {
        prover = prover.create_orchard_proof(UnwrapErr(SysRng), pk).map_err(|e| anyhow!("Orchard proof: {e:?}"))?;
    }
    if prover.requires_ironwood_proof() {
        prover = prover.create_ironwood_proof(UnwrapErr(SysRng), pk).map_err(|e| anyhow!("Ironwood proof: {e:?}"))?;
    }
    prover.finish().serialize().map_err(|e| anyhow!("serializing: {e:?}"))
}
