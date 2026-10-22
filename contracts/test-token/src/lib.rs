//! Testnet-only NEP-141 token standing in for USDC (6 decimals). Anyone can mint a
//! capped amount to themselves through `faucet`, so testers can bond, add liquidity and
//! receive payouts without depending on a third-party stablecoin. Never deploy to mainnet.

use near_contract_standards::fungible_token::metadata::{FungibleTokenMetadata, FungibleTokenMetadataProvider, FT_METADATA_SPEC};
use near_contract_standards::fungible_token::{FungibleToken, FungibleTokenCore, FungibleTokenResolver};
use near_contract_standards::storage_management::{StorageBalance, StorageBalanceBounds, StorageManagement};
use near_sdk::borsh::BorshSerialize;
use near_sdk::collections::LazyOption;
use near_sdk::json_types::U128;
use near_sdk::{env, log, near, require, AccountId, BorshStorageKey, NearToken, PanicOnDefault, PromiseOrValue};

/// Most a single faucet call mints: 10,000 tokens.
const FAUCET_MAX: u128 = 10_000 * 1_000_000;

#[derive(BorshSerialize, BorshStorageKey)]
#[borsh(crate = "near_sdk::borsh")]
enum StorageKey {
    FungibleToken,
    Metadata,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Contract {
    token: FungibleToken,
    metadata: LazyOption<FungibleTokenMetadata>,
}

#[near]
impl Contract {
    #[init]
    pub fn new(symbol: String, name: String) -> Self {
        let metadata = FungibleTokenMetadata {
            spec: FT_METADATA_SPEC.to_string(),
            name,
            symbol,
            icon: None,
            reference: None,
            reference_hash: None,
            decimals: 6,
        };
        metadata.assert_valid();
        Self { token: FungibleToken::new(StorageKey::FungibleToken), metadata: LazyOption::new(StorageKey::Metadata, Some(&metadata)) }
    }

    /// Mints up to 10,000 tokens to the caller, registering them first if needed.
    /// Attach enough NEAR to cover storage registration on the first call.
    #[payable]
    pub fn faucet(&mut self, amount: U128) -> U128 {
        require!(amount.0 > 0 && amount.0 <= FAUCET_MAX, "faucet amount must be between 1 and 10,000 tokens");
        let account = env::predecessor_account_id();
        if !self.token.accounts.contains_key(&account) {
            let min = self.token.storage_balance_bounds().min;
            require!(env::attached_deposit() >= min, "attach the storage deposit to register");
            self.token.internal_register_account(&account);
        }
        self.token.internal_deposit(&account, amount.0);
        near_contract_standards::fungible_token::events::FtMint { owner_id: &account, amount, memo: Some("faucet") }.emit();
        U128(self.token.internal_unwrap_balance_of(&account))
    }
}

#[near]
impl FungibleTokenCore for Contract {
    #[payable]
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>) {
        self.token.ft_transfer(receiver_id, amount, memo)
    }

    #[payable]
    fn ft_transfer_call(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>, msg: String) -> PromiseOrValue<U128> {
        self.token.ft_transfer_call(receiver_id, amount, memo, msg)
    }

    fn ft_total_supply(&self) -> U128 {
        self.token.ft_total_supply()
    }

    fn ft_balance_of(&self, account_id: AccountId) -> U128 {
        self.token.ft_balance_of(account_id)
    }
}

#[near]
impl FungibleTokenResolver for Contract {
    #[private]
    fn ft_resolve_transfer(&mut self, sender_id: AccountId, receiver_id: AccountId, amount: U128) -> U128 {
        let (used, burned) = self.token.internal_ft_resolve_transfer(&sender_id, receiver_id, amount);
        if burned > 0 {
            log!("Account @{} burned {}", sender_id, burned);
        }
        used.into()
    }
}

#[near]
impl StorageManagement for Contract {
    #[payable]
    fn storage_deposit(&mut self, account_id: Option<AccountId>, registration_only: Option<bool>) -> StorageBalance {
        self.token.storage_deposit(account_id, registration_only)
    }

    #[payable]
    fn storage_withdraw(&mut self, amount: Option<NearToken>) -> StorageBalance {
        self.token.storage_withdraw(amount)
    }

    #[payable]
    fn storage_unregister(&mut self, force: Option<bool>) -> bool {
        #[allow(unused_variables)]
        if let Some((account_id, balance)) = self.token.internal_storage_unregister(force) {
            log!("Closed @{} with {}", account_id, balance);
            true
        } else {
            false
        }
    }

    fn storage_balance_bounds(&self) -> StorageBalanceBounds {
        self.token.storage_balance_bounds()
    }

    fn storage_balance_of(&self, account_id: AccountId) -> Option<StorageBalance> {
        self.token.storage_balance_of(account_id)
    }
}

#[near]
impl FungibleTokenMetadataProvider for Contract {
    fn ft_metadata(&self) -> FungibleTokenMetadata {
        self.metadata.get().unwrap()
    }
}
