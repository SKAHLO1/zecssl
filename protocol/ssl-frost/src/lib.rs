//! Rerandomized FROST over RedPallas, as ZEC SSL runs it between the coordinator and the
//! operators. Everything crosses the wire as hex of FROST's own serialization.
//!
//! Round 1: each selected operator commits (fresh nonces, kept in memory, used once).
//! Round 2: the coordinator sends a signing package (the commitments plus the message);
//! each operator checks the message is the sighash *it* computed, then signs with the
//! randomizer (the spend's alpha) *it* computed from the transaction.
//! Aggregate: the coordinator combines the shares into one RedPallas signature, valid
//! under the group key rerandomized by alpha, which is what the Zcash spend needs.
//!
//! The coordinator only ever holds the public key package.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use reddsa::frost::redpallas as frost;
use reddsa::frost::redpallas::keys::{KeyPackage, PublicKeyPackage};
use reddsa::frost::redpallas::round1::SigningCommitments;
pub use reddsa::frost::redpallas::round1::SigningNonces;
use reddsa::frost::redpallas::round2::SignatureShare;
use reddsa::frost::redpallas::{Identifier, Randomizer, RandomizedParams, SigningPackage};
use serde::Deserialize;

fn de<T>(what: &str, hex_str: &str, f: impl FnOnce(&[u8]) -> Result<T, frost::Error>) -> Result<T> {
    let bytes = hex::decode(hex_str.trim()).with_context(|| format!("{what} is not hex"))?;
    f(&bytes).map_err(|e| anyhow!("invalid {what}: {e}"))
}

#[derive(Deserialize)]
struct FrostClientConfig {
    group: BTreeMap<String, FrostClientGroup>,
}

#[derive(Deserialize)]
struct FrostClientGroup {
    public_key_package: String,
    key_package: String,
}

/// An operator's share of the treasury key.
pub struct Share {
    key_package: KeyPackage,
    public: PublicKeyPackage,
}

impl Share {
    pub fn from_hex(key_package: &str, public_key_package: &str) -> Result<Self> {
        Ok(Self {
            key_package: de("key package", key_package, KeyPackage::deserialize)?,
            public: de("public key package", public_key_package, PublicKeyPackage::deserialize)?,
        })
    }

    /// Loads the share for `group_key` (hex) from a frost-client config file.
    pub fn from_frost_client_config(path: &Path, group_key: &str) -> Result<Self> {
        let cfg: FrostClientConfig = toml::from_str(&std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?)?;
        let g = cfg.group.get(group_key).ok_or_else(|| anyhow!("{} has no group {group_key}", path.display()))?;
        Self::from_hex(&g.key_package, &g.public_key_package)
    }

    pub fn identifier_hex(&self) -> String {
        hex::encode(self.key_package.identifier().serialize())
    }

    pub fn group_key_hex(&self) -> String {
        public_group_key_hex(&self.public)
    }

    /// The key package and public key package as hex, the form `from_hex` reads. The key
    /// package is secret.
    pub fn to_hex(&self) -> Result<(String, String)> {
        Ok((
            hex::encode(self.key_package.serialize().map_err(|e| anyhow!("{e}"))?),
            hex::encode(self.public.serialize().map_err(|e| anyhow!("{e}"))?),
        ))
    }

    /// Round 1: fresh nonces (keep in memory, use once) and the commitment to send.
    pub fn commit(&self) -> (SigningNonces, String) {
        let mut rng = rand_core::OsRng;
        let (nonces, commitments) = frost::round1::commit(self.key_package.signing_share(), &mut rng);
        let bytes = commitments.serialize().expect("commitments serialize");
        (nonces, hex::encode(bytes))
    }

    /// Round 2: signs `signing_package` only if its message is `expected_message`, with
    /// the randomizer `alpha` the caller derived itself. Consumes the nonces.
    pub fn sign(&self, signing_package: &str, nonces: SigningNonces, expected_message: &[u8], alpha: &[u8; 32]) -> Result<String> {
        let package = de("signing package", signing_package, SigningPackage::deserialize)?;
        if package.message() != expected_message {
            bail!("signing package message is not the sighash this operator computed");
        }
        if !package.signing_commitments().contains_key(self.key_package.identifier()) {
            bail!("signing package does not include this operator's commitment");
        }
        let randomizer = Randomizer::deserialize(alpha).map_err(|e| anyhow!("invalid randomizer: {e}"))?;
        let share = frost::round2::sign(&package, &nonces, &self.key_package, randomizer).map_err(|e| anyhow!("signing: {e}"))?;
        Ok(hex::encode(share.serialize()))
    }
}

pub fn public_group_key_hex(public: &PublicKeyPackage) -> String {
    hex::encode(public.verifying_key().serialize().expect("verifying key serializes"))
}

/// The coordinator's side: public data only.
pub struct Coordinator {
    public: PublicKeyPackage,
}

impl Coordinator {
    pub fn from_hex(public_key_package: &str) -> Result<Self> {
        Ok(Self { public: de("public key package", public_key_package, PublicKeyPackage::deserialize)? })
    }

    /// Reads the public key package from a frost-client config (any operator's: the public
    /// package is the same for all, and nothing secret is read).
    pub fn from_frost_client_config(path: &Path, group_key: &str) -> Result<Self> {
        let cfg: FrostClientConfig = toml::from_str(&std::fs::read_to_string(path)?)?;
        let g = cfg.group.get(group_key).ok_or_else(|| anyhow!("{} has no group {group_key}", path.display()))?;
        Self::from_hex(&g.public_key_package)
    }

    pub fn group_key_hex(&self) -> String {
        public_group_key_hex(&self.public)
    }

    pub fn public_key_package_hex(&self) -> String {
        hex::encode(self.public.serialize().expect("public key package serializes"))
    }

    /// Builds the signing package for one message from the signers' commitments
    /// (identifier hex -> commitments hex).
    pub fn signing_package(&self, commitments: &BTreeMap<String, String>, message: &[u8]) -> Result<String> {
        let mut map = BTreeMap::new();
        for (id, c) in commitments {
            let id = de("identifier", id, Identifier::deserialize)?;
            if !self.public.verifying_shares().contains_key(&id) {
                bail!("commitment from a party outside the group");
            }
            map.insert(id, de("commitments", c, SigningCommitments::deserialize)?);
        }
        Ok(hex::encode(SigningPackage::new(map, message).serialize().map_err(|e| anyhow!("{e}"))?))
    }

    /// Verifies every share and combines them into a RedPallas signature valid under the
    /// group key rerandomized by `alpha`. Returns the 64-byte signature as hex.
    pub fn aggregate(&self, signing_package: &str, shares: &BTreeMap<String, String>, alpha: &[u8; 32]) -> Result<String> {
        let package = de("signing package", signing_package, SigningPackage::deserialize)?;
        let mut map = BTreeMap::new();
        for (id, s) in shares {
            map.insert(de("identifier", id, Identifier::deserialize)?, de("signature share", s, SignatureShare::deserialize)?);
        }
        let randomizer = Randomizer::deserialize(alpha).map_err(|e| anyhow!("invalid randomizer: {e}"))?;
        let params = RandomizedParams::from_randomizer(self.public.verifying_key(), randomizer);
        let sig = frost::aggregate(&package, &map, &self.public, &params).map_err(|e| anyhow!("aggregating: {e}"))?;
        Ok(hex::encode(sig.serialize().map_err(|e| anyhow!("{e}"))?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reddsa::frost::redpallas::keys::{generate_with_dealer, IdentifierList};

    /// A full 2-of-3 round with a trusted-dealer key: shares verify and aggregate, and a
    /// tampered message is refused by the signer before it signs.
    #[test]
    fn two_of_three_round_produces_a_valid_rerandomized_signature() {
        let mut rng = rand_core::OsRng;
        let (shares, public) = generate_with_dealer(3, 2, IdentifierList::Default, &mut rng).unwrap();
        let public_hex = hex::encode(public.serialize().unwrap());
        let ops: Vec<Share> = shares
            .into_values()
            .map(|s| {
                let kp = KeyPackage::try_from(s).unwrap();
                Share::from_hex(&hex::encode(kp.serialize().unwrap()), &public_hex).unwrap()
            })
            .collect();
        let coordinator = Coordinator::from_hex(&public_hex).unwrap();

        let message = b"sighash bytes";
        // A valid Pallas scalar as the randomizer (small values are canonical).
        let mut alpha = [0u8; 32];
        alpha[0] = 42;

        let signers = &ops[..2];
        let mut nonces = vec![];
        let mut commitments = BTreeMap::new();
        for op in signers {
            let (n, c) = op.commit();
            nonces.push(n);
            commitments.insert(op.identifier_hex(), c);
        }
        let package = coordinator.signing_package(&commitments, message).unwrap();

        // A signer refuses a package whose message is not the sighash it computed.
        let (n_bad, _) = signers[0].commit();
        assert!(signers[0].sign(&package, n_bad, b"other message", &alpha).is_err());

        let mut shares = BTreeMap::new();
        for (op, n) in signers.iter().zip(nonces) {
            shares.insert(op.identifier_hex(), op.sign(&package, n, message, &alpha).unwrap());
        }
        let sig = coordinator.aggregate(&package, &shares, &alpha).unwrap();
        assert_eq!(sig.len(), 128);
    }
}
