//! A small NEAR client for ZEC SSL services: JSON-RPC view calls, and function-call
//! transactions signed with an ed25519 key.
//!
//! Transactions use NEAR's v0 transaction encoding (borsh), which every NEAR node
//! accepts: the signer hashes the borsh bytes with SHA-256 and signs the digest. This
//! avoids pulling `near-primitives` into a dependency tree shared with the Zcash crates.

use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use borsh::BorshSerialize;
use ed25519_dalek::{Signer as _, SigningKey};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

pub const TGAS: u64 = 1_000_000_000_000;

// ---------- wire format ----------

#[derive(BorshSerialize)]
struct PublicKey {
    key_type: u8, // 0 = ED25519
    data: [u8; 32],
}

#[derive(BorshSerialize)]
struct FunctionCall {
    method_name: String,
    args: Vec<u8>,
    gas: u64,
    deposit: u128,
}

#[derive(BorshSerialize)]
enum Action {
    _CreateAccount,
    _DeployContract,
    FunctionCall(Box<FunctionCall>),
}

#[derive(BorshSerialize)]
struct Transaction {
    signer_id: String,
    public_key: PublicKey,
    nonce: u64,
    receiver_id: String,
    block_hash: [u8; 32],
    actions: Vec<Action>,
}

#[derive(BorshSerialize)]
struct Signature {
    key_type: u8,
    data: [u8; 64],
}

#[derive(BorshSerialize)]
struct SignedTransaction {
    transaction: Transaction,
    signature: Signature,
}

// ---------- keys ----------

/// An account and the full-access or function-call key it signs with.
pub struct Signer {
    pub account_id: String,
    key: SigningKey,
}

impl Signer {
    /// Parses a NEAR secret key (`ed25519:<base58 of 64 bytes>`).
    pub fn new(account_id: impl Into<String>, secret: &str) -> Result<Self> {
        let encoded = secret.strip_prefix("ed25519:").ok_or_else(|| anyhow!("only ed25519 keys are supported"))?;
        let bytes = bs58::decode(encoded).into_vec().context("secret key is not base58")?;
        let seed: [u8; 32] = bytes.get(..32).and_then(|s| s.try_into().ok()).ok_or_else(|| anyhow!("secret key is too short"))?;
        let key = SigningKey::from_bytes(&seed);
        if bytes.len() == 64 && bytes[32..] != key.verifying_key().to_bytes() {
            bail!("secret key's public half does not match its seed");
        }
        Ok(Self { account_id: account_id.into(), key })
    }

    /// Loads a NEAR CLI legacy keychain file: `~/.near-credentials/<network>/<account>.json`
    /// or `.../<account>/ed25519_<key>.json`. near-cli-rs omits `account_id` from the file,
    /// so it falls back to the file or folder name.
    pub fn from_credentials_file(path: &Path) -> Result<Self> {
        let v: Value = serde_json::from_slice(&std::fs::read(path).with_context(|| format!("reading {}", path.display()))?)?;
        let account = match v["account_id"].as_str() {
            Some(a) => a.to_string(),
            None => {
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
                let name = if stem.starts_with("ed25519_") {
                    path.parent().and_then(|p| p.file_name()).and_then(|s| s.to_str()).unwrap_or_default()
                } else {
                    stem
                };
                anyhow::ensure!(!name.is_empty(), "cannot tell the account from {}", path.display());
                name.to_string()
            }
        };
        let secret = v["private_key"].as_str().or_else(|| v["secret_key"].as_str()).ok_or_else(|| anyhow!("credentials file has no private_key"))?;
        Self::new(account, secret)
    }

    pub fn public_key(&self) -> String {
        format!("ed25519:{}", bs58::encode(self.key.verifying_key().to_bytes()).into_string())
    }
}

// ---------- client ----------

/// What a transaction did.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub tx_hash: String,
    /// The decoded return value, if the call succeeded and returned one.
    pub value: Option<Value>,
    /// The failure, if the call panicked.
    pub failure: Option<String>,
}

impl Outcome {
    pub fn ok(&self) -> bool {
        self.failure.is_none()
    }
}

pub struct Near {
    rpc: String,
    http: reqwest::Client,
    /// Serializes this process's transactions so nonces never collide.
    nonce_lock: Mutex<()>,
}

impl Near {
    pub fn new(rpc_url: impl Into<String>) -> Self {
        Self { rpc: rpc_url.into(), http: reqwest::Client::new(), nonce_lock: Mutex::new(()) }
    }

    async fn rpc(&self, method: &str, params: Value) -> Result<Value> {
        let res: Value = self
            .http
            .post(&self.rpc)
            .json(&json!({ "jsonrpc": "2.0", "id": "ssl", "method": method, "params": params }))
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if let Some(err) = res.get("error") {
            bail!("{method}: {}", err.get("data").or(err.get("cause")).unwrap_or(err));
        }
        Ok(res["result"].clone())
    }

    /// Calls a view method and decodes its JSON result.
    pub async fn view<T: DeserializeOwned>(&self, contract: &str, method: &str, args: Value) -> Result<T> {
        let result = self
            .rpc(
                "query",
                json!({
                    "request_type": "call_function",
                    "finality": "final",
                    "account_id": contract,
                    "method_name": method,
                    "args_base64": B64.encode(serde_json::to_vec(&args)?),
                }),
            )
            .await?;
        if let Some(err) = result.get("error") {
            bail!("{contract}.{method}: {err}");
        }
        let bytes: Vec<u8> = serde_json::from_value(result["result"].clone()).context("view result has no bytes")?;
        serde_json::from_slice(&bytes).with_context(|| format!("decoding {contract}.{method}"))
    }

    /// Signs and sends one function call, waiting until it is final.
    pub async fn call(&self, signer: &Signer, contract: &str, method: &str, args: Value, gas: u64, deposit: u128) -> Result<Outcome> {
        let _guard = self.nonce_lock.lock().await;
        let access_key = self
            .rpc(
                "query",
                json!({ "request_type": "view_access_key", "finality": "final", "account_id": signer.account_id, "public_key": signer.public_key() }),
            )
            .await
            .context("loading the signer's access key")?;
        let nonce = access_key["nonce"].as_u64().ok_or_else(|| anyhow!("access key has no nonce"))? + 1;
        let block = self.rpc("block", json!({ "finality": "final" })).await?;
        let block_hash: [u8; 32] = bs58::decode(block["header"]["hash"].as_str().unwrap_or_default())
            .into_vec()?
            .try_into()
            .map_err(|_| anyhow!("block hash is not 32 bytes"))?;

        let tx = Transaction {
            signer_id: signer.account_id.clone(),
            public_key: PublicKey { key_type: 0, data: signer.key.verifying_key().to_bytes() },
            nonce,
            receiver_id: contract.to_string(),
            block_hash,
            actions: vec![Action::FunctionCall(Box::new(FunctionCall {
                method_name: method.to_string(),
                args: serde_json::to_vec(&args)?,
                gas,
                deposit,
            }))],
        };
        let digest = Sha256::digest(borsh::to_vec(&tx)?);
        let signature = Signature { key_type: 0, data: signer.key.sign(&digest).to_bytes() };
        let tx_hash = bs58::encode(digest).into_string();
        let signed = borsh::to_vec(&SignedTransaction { transaction: tx, signature })?;

        let result = self
            .rpc("send_tx", json!({ "signed_tx_base64": B64.encode(signed), "wait_until": "FINAL" }))
            .await
            .with_context(|| format!("sending {contract}.{method} (tx {tx_hash})"))?;
        Ok(decode_outcome(tx_hash, &result))
    }
}

fn decode_outcome(tx_hash: String, result: &Value) -> Outcome {
    let status = &result["status"];
    if let Some(failure) = status.get("Failure") {
        // Surface the contract's panic message when there is one.
        let msg = failure
            .pointer("/ActionError/kind/FunctionCallError/ExecutionError")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| failure.to_string());
        return Outcome { tx_hash, value: None, failure: Some(msg) };
    }
    // A receipt can fail even when the transaction's first action succeeded.
    if let Some(receipts) = result["receipts_outcome"].as_array() {
        for r in receipts {
            if let Some(f) = r.pointer("/outcome/status/Failure") {
                let msg = f
                    .pointer("/ActionError/kind/FunctionCallError/ExecutionError")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| f.to_string());
                return Outcome { tx_hash, value: None, failure: Some(msg) };
            }
        }
    }
    let value = status
        .get("SuccessValue")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .and_then(|s| B64.decode(s).ok())
        .and_then(|b| serde_json::from_slice(&b).ok());
    Outcome { tx_hash, value, failure: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A throwaway test key (never funded).
    const SEED: [u8; 32] = [7u8; 32];

    fn secret() -> String {
        let key = SigningKey::from_bytes(&SEED);
        let mut full = SEED.to_vec();
        full.extend_from_slice(&key.verifying_key().to_bytes());
        format!("ed25519:{}", bs58::encode(full).into_string())
    }

    #[test]
    fn parses_and_round_trips_a_near_secret_key() {
        let s = Signer::new("alice.testnet", &secret()).unwrap();
        let expected = format!("ed25519:{}", bs58::encode(SigningKey::from_bytes(&SEED).verifying_key().to_bytes()).into_string());
        assert_eq!(s.public_key(), expected);
    }

    #[test]
    fn rejects_a_mismatched_public_half() {
        let mut full = SEED.to_vec();
        full.extend_from_slice(&[0u8; 32]);
        let bad = format!("ed25519:{}", bs58::encode(full).into_string());
        assert!(Signer::new("alice.testnet", &bad).is_err());
    }

    /// The v0 encoding's layout: lengths are u32 little-endian, the FunctionCall action is
    /// enum index 2, and the deposit is a u128.
    #[test]
    fn function_call_encoding_matches_the_protocol_layout() {
        let tx = Transaction {
            signer_id: "a".into(),
            public_key: PublicKey { key_type: 0, data: [1; 32] },
            nonce: 5,
            receiver_id: "b".into(),
            block_hash: [2; 32],
            actions: vec![Action::FunctionCall(Box::new(FunctionCall { method_name: "m".into(), args: vec![9], gas: 3, deposit: 4 }))],
        };
        let bytes = borsh::to_vec(&tx).unwrap();
        let mut expected = vec![1, 0, 0, 0, b'a', 0];
        expected.extend([1; 32]);
        expected.extend(5u64.to_le_bytes());
        expected.extend([1, 0, 0, 0, b'b']);
        expected.extend([2; 32]);
        expected.extend([1, 0, 0, 0, 2, 1, 0, 0, 0, b'm', 1, 0, 0, 0, 9]);
        expected.extend(3u64.to_le_bytes());
        expected.extend(4u128.to_le_bytes());
        assert_eq!(bytes, expected);
    }

    #[test]
    fn decodes_success_and_failure_outcomes() {
        let ok = json!({ "status": { "SuccessValue": B64.encode(b"\"open\"") }, "receipts_outcome": [] });
        assert_eq!(decode_outcome("h".into(), &ok).value, Some(json!("open")));
        let bad = json!({ "status": { "Failure": { "ActionError": { "kind": { "FunctionCallError": { "ExecutionError": "Smart contract panicked: order is not open" } } } } } });
        assert_eq!(decode_outcome("h".into(), &bad).failure.as_deref(), Some("Smart contract panicked: order is not open"));
    }
}
