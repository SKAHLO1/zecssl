//! The NEAR vault contract (contracts/ssl), as the services use it.

use anyhow::Result;
use serde::Deserialize;
use serde_json::json;
use ssl_near::{Near, Outcome, Signer, TGAS};
use ssl_policy::OrderTerms;

#[derive(Clone, Debug, Deserialize)]
pub struct VaultConfig {
    pub token: String,
    pub token_symbol: String,
    pub token_decimals: u8,
    pub threshold: u8,
}

#[derive(Clone, Debug, Deserialize)]
pub struct VaultOrder {
    pub id: String,
    pub state: String,
    pub terms: OrderTerms,
    pub approvers: Vec<String>,
    pub release_approvals: Vec<String>,
    pub refund_approvals: Vec<String>,
    pub release_deadline: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct VaultOperator {
    pub account_id: String,
    pub active: bool,
    pub bond: String,
    pub unbonding: String,
}

pub struct Vault {
    pub near: Near,
    pub contract: String,
}

impl Vault {
    pub fn new(rpc: &str, contract: &str) -> Self {
        Self { near: Near::new(rpc), contract: contract.to_string() }
    }

    pub async fn config(&self) -> Result<VaultConfig> {
        self.near.view(&self.contract, "get_config", json!({})).await
    }

    pub async fn order(&self, id: &str) -> Result<Option<VaultOrder>> {
        self.near.view(&self.contract, "get_order", json!({ "order_id": id })).await
    }

    pub async fn operator(&self, account: &str) -> Result<Option<VaultOperator>> {
        self.near.view(&self.contract, "get_operator", json!({ "account_id": account })).await
    }

    pub async fn approve_order(&self, signer: &Signer, id: &str, terms: &OrderTerms) -> Result<Outcome> {
        self.near.call(signer, &self.contract, "approve_order", json!({ "order_id": id, "terms": terms }), 50 * TGAS, 0).await
    }

    /// Release triggers a token transfer and its callback, so it needs more gas.
    pub async fn approve_release(&self, signer: &Signer, id: &str) -> Result<Outcome> {
        self.near.call(signer, &self.contract, "approve_release", json!({ "order_id": id }), 100 * TGAS, 0).await
    }
}
