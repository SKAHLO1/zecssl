//! External spend authorization for ZEC SSL treasury transactions.
//!
//! The treasury's spend-authorizing key exists only as FROST shares, so a PCZT is
//! signed in three steps:
//!
//! 1. [`prepare`] reads a proven-or-unproven PCZT and returns the sighash, the
//!    randomizer (`alpha`) of every spend awaiting a signature, and a summary of the
//!    transaction's effects that operators check before they agree to sign.
//! 2. Operators run a rerandomized FROST round over (sighash, alpha) for each spend.
//! 3. [`apply`] injects the resulting RedPallas signatures. Each one is verified
//!    against the spend's randomized key before it is accepted.
//!
//! Ironwood and Orchard bundles are both Orchard-shaped; a v6 transaction can carry
//! spends in either (an Orchard spend migrating into Ironwood, for example), so every
//! spend is addressed by pool and action index.

use anyhow::{anyhow, bail, Context, Result};
use ff::PrimeField;
use orchard::primitives::redpallas::{self, SpendAuth};
use pczt::{
    roles::low_level_signer::{OrchardParseError, Signer},
    Pczt,
};
use serde::{Deserialize, Serialize};
use zcash_primitives::transaction::{
    sighash::SignableInput, sighash_v5::v5_signature_hash, sighash_v6::v6_signature_hash,
    txid::TxIdDigester, TxVersion,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pool {
    Orchard,
    Ironwood,
}

/// A spend that needs an external spend-authorization signature.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpendToSign {
    pub pool: Pool,
    /// Action index within that pool's bundle.
    pub index: usize,
    /// The randomizer, hex (32 bytes). FROST signs with this as the rerandomizer.
    pub alpha: String,
}

/// A shielded output as the PCZT describes it to signers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShieldedOutput {
    pub pool: Pool,
    pub index: usize,
    /// Zatoshis. `None` if the PCZT has been redacted.
    pub value: Option<u64>,
    /// The encoded address the creator recorded for this output, when present.
    pub address: Option<String>,
}

/// What a transaction does, in the terms the operator policy checks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effects {
    pub tx_version: String,
    pub transparent_inputs: usize,
    pub transparent_outputs: usize,
    pub sapling_spends: usize,
    pub sapling_outputs: usize,
    pub outputs: Vec<ShieldedOutput>,
}

impl Effects {
    /// The PRD's hard line: a treasury transaction never touches a transparent address.
    pub fn is_fully_shielded(&self) -> bool {
        self.transparent_inputs == 0 && self.transparent_outputs == 0
    }
}

/// Everything signers need: what to sign, and what they would be agreeing to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SigningRequest {
    /// Hex (32 bytes). The message every FROST round signs.
    pub sighash: String,
    pub spends: Vec<SpendToSign>,
    pub effects: Effects,
}

/// A signature produced by a FROST round, for one spend.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpendSignature {
    pub pool: Pool,
    pub index: usize,
    /// Hex (64 bytes), RedPallas.
    pub signature: String,
}

fn pools_for(version: TxVersion) -> &'static [Pool] {
    match version {
        TxVersion::V6 => &[Pool::Orchard, Pool::Ironwood],
        _ => &[Pool::Orchard],
    }
}

/// The shielded sighash. Ironwood lives only in v6 transactions, whose sighash commits to
/// the Ironwood bundle; using the v5 hash would sign successfully here and then be
/// rejected at broadcast.
fn sighash(pczt: &Pczt) -> Result<(TxVersion, [u8; 32])> {
    let tx_data = pczt
        .clone()
        .into_effects()
        .map_err(|e| anyhow!("PCZT lacks the data to compute its effects: {e:?}"))?;
    let txid_parts = tx_data.digest(TxIdDigester);
    let version = tx_data.version();
    let hash = match version {
        TxVersion::V6 => v6_signature_hash(&tx_data, &SignableInput::Shielded, &txid_parts),
        TxVersion::V5 => v5_signature_hash(&tx_data, &SignableInput::Shielded, &txid_parts),
        other => bail!("unsupported transaction version {other:?}"),
    };
    let bytes: [u8; 32] = hash.as_ref().try_into().context("sighash is not 32 bytes")?;
    Ok((version, bytes))
}

/// Runs `f` over the bundle of `pool`, through the low-level signer.
fn with_bundle<E>(
    signer: Signer,
    pool: Pool,
    f: impl FnOnce(&mut orchard::pczt::Bundle) -> Result<(), E>,
) -> Result<Signer, E>
where
    E: From<OrchardParseError>,
{
    match pool {
        Pool::Orchard => signer.sign_orchard_with(|_, bundle, _| f(bundle)),
        Pool::Ironwood => signer.sign_ironwood_with(|_, bundle, _| f(bundle)),
    }
}

pub fn prepare(pczt_bytes: &[u8]) -> Result<SigningRequest> {
    let pczt = Pczt::parse(pczt_bytes).map_err(|e| anyhow!("invalid PCZT: {e:?}"))?;
    let (version, hash) = sighash(&pczt)?;

    let mut spends = vec![];
    let mut outputs = vec![];
    for &pool in pools_for(version) {
        let mut found_spends = vec![];
        let mut found_outputs = vec![];
        with_bundle(Signer::new(pczt.clone()), pool, |bundle| -> Result<(), OrchardParseError> {
            for (index, action) in bundle.actions().iter().enumerate() {
                let spend = action.spend();
                // Real spends await a signature; dummy spends were signed by the IO finalizer.
                if spend.spend_auth_sig().is_none() {
                    if let Some(alpha) = spend.alpha() {
                        found_spends.push(SpendToSign { pool, index, alpha: hex::encode(alpha.to_repr()) });
                    }
                }
                let output = action.output();
                let value = output.value().map(|v| v.inner());
                // Zero-valued outputs with no recorded address are padding.
                if value.unwrap_or(0) > 0 || output.user_address().is_some() {
                    found_outputs.push(ShieldedOutput { pool, index, value, address: output.user_address().clone() });
                }
            }
            Ok(())
        })
        .map_err(|e| anyhow!("reading the {pool:?} bundle: {e:?}"))?;
        spends.extend(found_spends);
        outputs.extend(found_outputs);
    }

    Ok(SigningRequest {
        sighash: hex::encode(hash),
        spends,
        effects: Effects {
            tx_version: format!("{version:?}"),
            transparent_inputs: pczt.transparent().inputs().len(),
            transparent_outputs: pczt.transparent().outputs().len(),
            sapling_spends: pczt.sapling().spends().len(),
            sapling_outputs: pczt.sapling().outputs().len(),
            outputs,
        },
    })
}

#[derive(Debug)]
enum ApplyError {
    // Both are only read through `Debug`, in the error message `apply` returns.
    Parse(#[allow(dead_code)] OrchardParseError),
    Signature(#[allow(dead_code)] String),
}

impl From<OrchardParseError> for ApplyError {
    fn from(e: OrchardParseError) -> Self {
        ApplyError::Parse(e)
    }
}

/// Injects externally produced signatures. Every awaiting spend must be covered.
pub fn apply(pczt_bytes: &[u8], signatures: &[SpendSignature]) -> Result<Vec<u8>> {
    let pczt = Pczt::parse(pczt_bytes).map_err(|e| anyhow!("invalid PCZT: {e:?}"))?;
    let request = prepare(pczt_bytes)?;
    let (version, hash) = sighash(&pczt)?;

    for spend in &request.spends {
        if !signatures.iter().any(|s| s.pool == spend.pool && s.index == spend.index) {
            bail!("missing signature for {:?} action {}", spend.pool, spend.index);
        }
    }

    let mut signer = Signer::new(pczt);
    for &pool in pools_for(version) {
        let mut sigs = vec![];
        for s in signatures.iter().filter(|s| s.pool == pool) {
            let bytes: [u8; 64] = hex::decode(&s.signature)?
                .try_into()
                .map_err(|_| anyhow!("signature for {pool:?} action {} is not 64 bytes", s.index))?;
            sigs.push((s.index, bytes));
        }
        if sigs.is_empty() {
            continue;
        }
        signer = with_bundle(signer, pool, |bundle| -> Result<(), ApplyError> {
            let actions = bundle.actions_mut();
            for (index, bytes) in &sigs {
                let action = actions
                    .get_mut(*index)
                    .ok_or_else(|| ApplyError::Signature(format!("no action {index}")))?;
                action
                    .apply_signature(hash, redpallas::Signature::<SpendAuth>::from(*bytes))
                    .map_err(|e| ApplyError::Signature(format!("action {index}: {e:?}")))?;
            }
            Ok(())
        })
        .map_err(|e| anyhow!("applying {pool:?} signatures: {e:?}"))?;
    }

    signer
        .finish()
        .serialize()
        .map_err(|e| anyhow!("serializing the signed PCZT: {e:?}"))
}

/// Checks a spend-authorization signature against the group key, rerandomized by the
/// spend's alpha: rk = ak + [alpha]G. This is the check `apply` relies on, exposed so an
/// operator can confirm a FROST round's output independently.
pub fn verify(ak_hex: &str, alpha_hex: &str, sighash_hex: &str, signature_hex: &str) -> Result<()> {
    let ak: [u8; 32] = hex::decode(ak_hex)?.try_into().map_err(|_| anyhow!("ak must be 32 bytes"))?;
    let alpha: [u8; 32] = hex::decode(alpha_hex)?.try_into().map_err(|_| anyhow!("alpha must be 32 bytes"))?;
    let sig: [u8; 64] = hex::decode(signature_hex)?.try_into().map_err(|_| anyhow!("signature must be 64 bytes"))?;
    let msg = hex::decode(sighash_hex)?;

    let ak = reddsa::VerificationKey::<reddsa::orchard::SpendAuth>::try_from(ak)
        .map_err(|e| anyhow!("ak is not a valid key: {e:?}"))?;
    let alpha = Option::<pasta_curves::pallas::Scalar>::from(pasta_curves::pallas::Scalar::from_repr(alpha))
        .ok_or_else(|| anyhow!("alpha is not a canonical Pallas scalar"))?;
    ak.randomize(&alpha)
        .verify(&msg, &reddsa::Signature::from(sig))
        .map_err(|e| anyhow!("signature does not verify: {e:?}"))
}

/// The finished transaction, extracted from a fully signed PCZT. Extraction verifies
/// every proof and signature; the transparent check runs on the final transaction, not
/// on the PCZT's description of it.
pub struct Extracted {
    pub txid: String,
    pub raw: Vec<u8>,
    pub transparent_inputs: usize,
    pub transparent_outputs: usize,
}

pub fn extract(pczt_bytes: &[u8]) -> Result<Extracted> {
    use pczt::roles::{spend_finalizer::SpendFinalizer, tx_extractor::TransactionExtractor};
    use rand::{rand_core::UnwrapErr, rngs::SysRng};

    let pczt = Pczt::parse(pczt_bytes).map_err(|e| anyhow!("invalid PCZT: {e:?}"))?;
    let finalized = SpendFinalizer::new(pczt)
        .finalize_spends()
        .map_err(|e| anyhow!("finalizing spends: {e:?}"))?;
    let tx = TransactionExtractor::new(finalized)
        .extract(UnwrapErr(SysRng))
        .map_err(|e| anyhow!("extracting the transaction: {e:?}"))?;

    let (transparent_inputs, transparent_outputs) = tx
        .transparent_bundle()
        .map(|b| (b.vin.len(), b.vout.len()))
        .unwrap_or((0, 0));
    let mut raw = vec![];
    tx.write(&mut raw)?;
    Ok(Extracted { txid: tx.txid().to_string(), raw, transparent_inputs, transparent_outputs })
}
