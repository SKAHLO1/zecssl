//! Operator bonds and t-of-n liquidity withdrawals.

use near_sdk::json_types::{U128, U64};
use near_sdk::{env, near, require, AccountId, Promise};

use crate::{events, now, validate_id, Contract, ContractExt, Withdrawal};

#[near]
impl Contract {
    /// Moves part of the caller's bond into unbonding. It stays slashable until
    /// `unbonding_delay_ns` has passed. Refused if the remaining bonds could no longer
    /// cover the value already in flight.
    pub fn request_unbond(&mut self, amount: U128) {
        let caller = env::predecessor_account_id();
        let delay = self.config.unbonding_delay_ns.0;
        let op = self.operators.get_mut(&caller).unwrap_or_else(|| env::panic_str("caller is not an operator"));
        require!(amount.0 > 0 && amount.0 <= op.bond.0, "invalid unbond amount");
        op.bond = U128(op.bond.0 - amount.0);
        op.unbonding = U128(op.unbonding.0 + amount.0);
        op.unlock_at = U64(now() + delay);
        // Unbonding funds still count as slashable, but an operator that drops below the
        // minimum bond stops being eligible, which can shrink capacity.
        require!(self.in_flight() <= self.max_in_flight(), "unbonding would leave in-flight value under-bonded");
        events::bond(&caller, "unbond_requested", amount.0);
    }

    /// Pays out the caller's unbonded funds once the delay has passed.
    pub fn withdraw_unbonded(&mut self) -> Promise {
        let caller = env::predecessor_account_id();
        let op = self.operators.get_mut(&caller).unwrap_or_else(|| env::panic_str("caller is not an operator"));
        require!(op.unbonding.0 > 0, "nothing is unbonding");
        require!(now() >= op.unlock_at.0, "unbonding delay has not passed");
        let amount = op.unbonding.0;
        op.unbonding = U128(0);
        op.unlock_at = U64(0);
        events::bond(&caller, "withdrawn", amount);
        self.payout(caller, amount, "zec-ssl unbond")
    }

    /// One operator's approval to withdraw vault liquidity. Executes when `threshold`
    /// operators approve the same id, receiver and amount.
    pub fn approve_withdrawal(&mut self, id: String, receiver: AccountId, amount: U128) -> Option<Promise> {
        self.require_not_paused();
        validate_id(&id);
        let caller = self.require_eligible_operator();
        let mut w = self.withdrawals.get(&id).cloned().unwrap_or(Withdrawal {
            id: id.clone(),
            receiver: receiver.clone(),
            amount,
            approvals: vec![],
            executed: false,
        });
        require!(!w.executed, "withdrawal already executed");
        require!(w.receiver == receiver && w.amount == amount, "withdrawal terms differ from earlier approvals");
        if !w.approvals.contains(&caller) {
            w.approvals.push(caller);
        }
        let ready = w.approvals.len() >= self.config.threshold as usize;
        if ready {
            require!(self.liquidity - self.reserved >= amount.0, "not enough unreserved liquidity");
            w.executed = true;
        }
        self.withdrawals.insert(id, w);
        if ready {
            self.liquidity -= amount.0;
            events::liquidity("withdrawn", &receiver, amount.0);
            return Some(self.payout(receiver, amount.0, "zec-ssl liquidity withdrawal"));
        }
        None
    }
}
