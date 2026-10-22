//! Token flows: everything arrives through `ft_transfer_call` and leaves through
//! `payout`. A failed outgoing transfer (for example, the receiver has no storage
//! registered with the token) is credited to `claimable` instead of being retried
//! blindly, and the receiver withdraws it later with `claim`.

use near_sdk::json_types::U128;
use near_sdk::serde_json;
use near_sdk::{env, ext_contract, near, require, AccountId, Gas, NearToken, Promise, PromiseError, PromiseOrValue};

use crate::{events, Contract, ContractExt, TransferMsg};

const GAS_FT_TRANSFER: Gas = Gas::from_tgas(10);
const GAS_ON_PAYOUT: Gas = Gas::from_tgas(10);

#[ext_contract(ext_ft)]
#[allow(dead_code)]
trait FungibleToken {
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>);
}

#[near]
impl Contract {
    /// NEP-141 receiver. Returns the unused amount, which the token refunds to the sender.
    pub fn ft_on_transfer(&mut self, sender_id: AccountId, amount: U128, msg: String) -> PromiseOrValue<U128> {
        require!(env::predecessor_account_id() == self.config.token, "only the vault token is accepted");
        let msg: TransferMsg = serde_json::from_str(&msg).unwrap_or_else(|e| env::panic_str(&format!("invalid msg: {e}")));
        let unused = match msg {
            TransferMsg::Bond => {
                let op = self.operators.get_mut(&sender_id).unwrap_or_else(|| env::panic_str("sender is not an operator"));
                require!(op.active, "operator is not active");
                op.bond = U128(op.bond.0 + amount.0);
                events::bond(&sender_id, "bonded", amount.0);
                0
            }
            TransferMsg::Liquidity => {
                self.liquidity += amount.0;
                events::liquidity("deposited", &sender_id, amount.0);
                0
            }
            TransferMsg::Dispute { order_id } => self.open_dispute(sender_id, order_id, amount.0),
            TransferMsg::LockForZec { order_id, zec_address, zec_amount_zat, deadline } => {
                self.lock_for_zec(sender_id, order_id, zec_address, zec_amount_zat, deadline, amount.0);
                0
            }
            TransferMsg::Challenge { order_id } => self.challenge(sender_id, order_id, amount.0),
        };
        PromiseOrValue::Value(U128(unused))
    }

    /// Withdraws payouts that failed earlier.
    pub fn claim(&mut self) -> Promise {
        let caller = env::predecessor_account_id();
        let amount = self.claimable.remove(&caller).unwrap_or(0);
        require!(amount > 0, "nothing to claim");
        self.claimable_total -= amount;
        self.payout(caller, amount, "zec-ssl claim")
    }

    #[private]
    pub fn on_payout(&mut self, receiver: AccountId, amount: U128, #[callback_result] result: Result<(), PromiseError>) -> bool {
        let ok = result.is_ok();
        if !ok {
            *self.claimable.entry(receiver.clone()).or_insert(0) += amount.0;
            self.claimable_total += amount.0;
        }
        events::payout(&receiver, amount.0, ok);
        ok
    }
}

impl Contract {
    /// Sends `amount` of the vault token. Callers must already have debited it from
    /// the relevant balance; on failure it becomes claimable by `receiver`.
    pub(crate) fn payout(&mut self, receiver: AccountId, amount: u128, memo: &str) -> Promise {
        ext_ft::ext(self.config.token.clone())
            .with_attached_deposit(NearToken::from_yoctonear(1))
            .with_static_gas(GAS_FT_TRANSFER)
            .ft_transfer(receiver.clone(), U128(amount), Some(memo.to_string()))
            .then(Self::ext(env::current_account_id()).with_static_gas(GAS_ON_PAYOUT).on_payout(receiver, U128(amount)))
    }
}
