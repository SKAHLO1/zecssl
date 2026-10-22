//! Arbiter governance: a t-of-n multisig over a fixed set of actions. Arbiters can resolve
//! disputes, slash, manage the operator set, change parameters and pause. They cannot
//! move vault liquidity or user escrow except as a dispute resolution prescribes.

use near_sdk::json_types::U64;
use near_sdk::{env, near, require, Promise};

use crate::{events, now, validate_config, Contract, ContractExt, GovAction, Proposal};

#[near]
impl Contract {
    /// Creates a proposal and counts it as the proposer's approval.
    pub fn propose(&mut self, action: GovAction) -> u64 {
        let caller = self.require_arbiter();
        let id = self.next_proposal_id;
        self.next_proposal_id += 1;
        self.proposals.insert(id, Proposal { id, action, approvals: vec![], executed: false, created_at: U64(now()) });
        events::governance(id, "proposed", &caller);
        self.approve_proposal(id);
        id
    }

    /// Approves a proposal; executes it when `arbiter_threshold` arbiters have approved.
    pub fn approve_proposal(&mut self, id: u64) {
        let caller = self.require_arbiter();
        let threshold = self.config.arbiter_threshold as usize;
        let proposal = self.proposals.get_mut(&id).unwrap_or_else(|| env::panic_str("unknown proposal"));
        require!(!proposal.executed, "proposal already executed");
        if proposal.approvals.contains(&caller) {
            return;
        }
        proposal.approvals.push(caller.clone());
        events::governance(id, "approved", &caller);
        if proposal.approvals.len() < threshold {
            return;
        }
        proposal.executed = true;
        let action = proposal.action.clone();
        events::governance(id, "executed", &caller);
        self.execute(action);
    }
}

impl Contract {
    fn require_arbiter(&self) -> near_sdk::AccountId {
        let caller = env::predecessor_account_id();
        require!(self.arbiters.contains(&caller), "caller is not an arbiter");
        caller
    }

    fn execute(&mut self, action: GovAction) {
        match action {
            GovAction::AddOperator { account_id } => self.add_operator_internal(account_id),
            GovAction::RemoveOperator { account_id } => {
                let op = self.operators.get_mut(&account_id).unwrap_or_else(|| env::panic_str("unknown operator"));
                op.active = false;
            }
            GovAction::Slash { operator, amount, reason: _ } => {
                let taken = self.slash_internal(&operator, amount.0);
                self.insurance += taken;
            }
            GovAction::ResolveDispute { order_id, upheld } => {
                for (receiver, amount, memo) in self.resolve_dispute(&order_id, upheld) {
                    if amount > 0 {
                        let _: Promise = self.payout(receiver, amount, &memo);
                    }
                }
            }
            GovAction::SetConfig { config } => {
                require!(config.token == self.config.token, "the vault token cannot be changed");
                validate_config(&config, self.operators.len() as usize, self.arbiters.len());
                self.config = config;
            }
            GovAction::Pause => self.paused = true,
            GovAction::Unpause => self.paused = false,
            GovAction::WithdrawInsurance { receiver, amount } => {
                require!(amount.0 <= self.insurance, "not enough insurance funds");
                self.insurance -= amount.0;
                let _: Promise = self.payout(receiver, amount.0, "zec-ssl insurance withdrawal");
            }
        }
    }
}
