//! Messages of a treasury signing round between the coordinator and the operators.
//! FROST values are hex of FROST's own serialization; the PCZT is base64.

use serde::{Deserialize, Serialize};

/// What a treasury spend is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoundKind {
    /// Direction 1: an underpaid or late deposit goes back to the user's refund address.
    Refund,
    /// Direction 2: the ZEC a user locked vault tokens for goes to their shielded address.
    Payout,
}

impl RoundKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RoundKind::Refund => "refund",
            RoundKind::Payout => "payout",
        }
    }
}

/// Round 1: here is the proven, unsigned transaction; check it and commit.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Round1Request {
    pub round_id: String,
    pub kind: RoundKind,
    pub order_id: String,
    pub pczt: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpendCommitment {
    pub pool: ssl_sign::Pool,
    pub index: usize,
    pub commitments: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Round1Response {
    /// This operator's FROST identifier.
    pub identifier: String,
    /// The sighash this operator computed; the coordinator must sign exactly this.
    pub sighash: String,
    pub commitments: Vec<SpendCommitment>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpendPackage {
    pub pool: ssl_sign::Pool,
    pub index: usize,
    pub signing_package: String,
}

/// Round 2: the signing packages built from the chosen signers' commitments.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Round2Request {
    pub round_id: String,
    pub packages: Vec<SpendPackage>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpendShare {
    pub pool: ssl_sign::Pool,
    pub index: usize,
    pub share: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Round2Response {
    pub identifier: String,
    pub shares: Vec<SpendShare>,
}

/// A v6 fully shielded transaction's txid is its sighash, byte-reversed (ZIP 244).
pub fn txid_from_sighash(sighash_hex: &str) -> Option<String> {
    let mut b = hex::decode(sighash_hex).ok()?;
    b.reverse();
    Some(hex::encode(b))
}

/// Converts ssl-sign's view of a transaction into the policy's.
pub fn refund_tx(effects: &ssl_sign::Effects) -> ssl_policy::RefundTx {
    ssl_policy::RefundTx {
        transparent_inputs: effects.transparent_inputs,
        transparent_outputs: effects.transparent_outputs,
        sapling_spends: effects.sapling_spends,
        sapling_outputs: effects.sapling_outputs,
        spent_zat: effects.spent_zat,
        outputs: effects
            .outputs
            .iter()
            .map(|o| ssl_policy::TxOutput {
                value: o.value,
                recipient: o.recipient.as_deref().and_then(|h| hex::decode(h).ok()).and_then(|b| <[u8; 43]>::try_from(b).ok()),
            })
            .collect(),
    }
}
