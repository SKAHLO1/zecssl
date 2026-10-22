//! ZEC SSL on NEAR.
//!
//! One contract holds the vault token (USDC) for three purposes: liquidity the vault pays
//! out, operator bonds, and direction-2 escrow. Operators act only through t-of-n on-chain
//! approvals, so every approval is attributable and slashable. Arbiters (a separate
//! t-of-n set) resolve disputes, slash, and manage the operator set; they cannot move
//! vault liquidity or escrow except through those rules.
//!
//! Safety properties this contract enforces:
//! - Nothing leaves the vault without `threshold` operator approvals of identical terms,
//!   or an executed arbiter decision.
//! - Value in flight never exceeds the combined bonds of the `threshold` smallest
//!   eligible operators, so any colluding quorum loses more than it could take.
//! - Unbonding funds stay slashable for longer than the release-plus-dispute period.
//! - Every token the contract holds is accounted for by purpose (`get_accounting`).

mod events;
mod governance;
mod operators;
mod orders;
mod token;
mod types;

#[cfg(test)]
mod tests;

pub use types::*;

use near_sdk::borsh::BorshSerialize;
use near_sdk::json_types::{U128, U64};
use near_sdk::store::{IterableMap, LookupMap, Vector};
use near_sdk::{env, near, require, AccountId, BorshStorageKey, PanicOnDefault};

#[derive(BorshStorageKey, BorshSerialize)]
#[borsh(crate = "near_sdk::borsh")]
enum StorageKey {
    Operators,
    Orders,
    OrderIds,
    Proposals,
    Withdrawals,
    Claimable,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Contract {
    config: Config,
    paused: bool,
    operators: IterableMap<AccountId, Operator>,
    arbiters: Vec<AccountId>,
    orders: IterableMap<OrderId, Order>,
    /// Order ids in creation order, for pagination.
    order_ids: Vector<OrderId>,
    proposals: IterableMap<u64, Proposal>,
    next_proposal_id: u64,
    withdrawals: LookupMap<String, Withdrawal>,
    /// Payouts whose transfer failed (e.g. receiver not registered with the token).
    claimable: LookupMap<AccountId, u128>,

    /// Vault-owned tokens available for direction-1 payouts (includes `reserved`).
    liquidity: u128,
    /// Part of `liquidity` set aside for open direction-1 orders.
    reserved: u128,
    /// Direction-2 user escrow.
    escrowed: u128,
    /// Slashed funds and forfeited challenge bonds.
    insurance: u128,
    /// Bonds posted by disputers and challengers, held until resolution.
    challenge_bonds: u128,
    claimable_total: u128,

    epoch_start: u64,
    epoch_volume: u128,
}

pub(crate) fn now() -> u64 {
    env::block_timestamp()
}

pub(crate) fn validate_id(id: &str) {
    require!(
        !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
        "id must be 1-64 characters of [A-Za-z0-9_-]"
    );
}

pub(crate) fn validate_config(c: &Config, operators: usize, arbiters: usize) {
    require!(c.threshold >= 1 && (c.threshold as usize) <= operators, "threshold must be between 1 and the number of operators");
    require!(c.arbiter_threshold >= 1 && (c.arbiter_threshold as usize) <= arbiters, "invalid arbiter threshold");
    require!(c.epoch_length_ns.0 > 0, "epoch length must be positive");
    require!(c.per_order_cap.0 > 0 && c.per_order_cap.0 <= c.epoch_cap.0, "per-order cap must be positive and within the epoch cap");
    let exposure = c.release_window_ns.0 as u128 + c.dispute_window_ns.0 as u128;
    require!(
        (c.unbonding_delay_ns.0 as u128) > exposure && c.unbonding_delay_ns.0 > c.challenge_window_ns.0,
        "unbonding delay must exceed the release + dispute windows and the challenge window"
    );
}

pub(crate) fn format_units(raw: u128, decimals: u8) -> String {
    let base = 10u128.pow(decimals as u32);
    let frac = raw % base;
    if decimals == 0 || frac == 0 {
        return (raw / base).to_string();
    }
    let frac = format!("{:0width$}", frac, width = decimals as usize);
    format!("{}.{}", raw / base, frac.trim_end_matches('0'))
}

#[near]
impl Contract {
    #[init]
    pub fn new(config: Config, operators: Vec<AccountId>, arbiters: Vec<AccountId>) -> Self {
        require!(!arbiters.is_empty(), "at least one arbiter is required");
        require!(operators.len() >= config.threshold as usize, "fewer operators than the threshold");
        validate_config(&config, operators.len(), arbiters.len());
        let mut this = Self {
            config,
            paused: false,
            operators: IterableMap::new(StorageKey::Operators),
            arbiters,
            orders: IterableMap::new(StorageKey::Orders),
            order_ids: Vector::new(StorageKey::OrderIds),
            proposals: IterableMap::new(StorageKey::Proposals),
            next_proposal_id: 0,
            withdrawals: LookupMap::new(StorageKey::Withdrawals),
            claimable: LookupMap::new(StorageKey::Claimable),
            liquidity: 0,
            reserved: 0,
            escrowed: 0,
            insurance: 0,
            challenge_bonds: 0,
            claimable_total: 0,
            epoch_start: now(),
            epoch_volume: 0,
        };
        for op in operators {
            this.add_operator_internal(op);
        }
        this
    }

    // ---------- views ----------

    pub fn get_config(&self) -> &Config {
        &self.config
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn get_arbiters(&self) -> &Vec<AccountId> {
        &self.arbiters
    }

    pub fn get_operators(&self) -> Vec<&Operator> {
        self.operators.values().collect()
    }

    pub fn get_operator(&self, account_id: AccountId) -> Option<&Operator> {
        self.operators.get(&account_id)
    }

    pub fn get_order(&self, order_id: OrderId) -> Option<&Order> {
        self.orders.get(&order_id)
    }

    /// Newest first.
    pub fn get_orders(&self, from_index: Option<u32>, limit: Option<u32>) -> Vec<&Order> {
        let len = self.order_ids.len();
        let from = from_index.unwrap_or(0).min(len);
        let limit = limit.unwrap_or(50).min(100);
        (from..len.min(from.saturating_add(limit)))
            .filter_map(|i| self.order_ids.get(len - 1 - i))
            .filter_map(|id| self.orders.get(id))
            .collect()
    }

    pub fn get_proposals(&self, from_index: Option<u64>, limit: Option<u64>) -> Vec<&Proposal> {
        let from = from_index.unwrap_or(0);
        let limit = limit.unwrap_or(50).min(100);
        (from..from.saturating_add(limit)).filter_map(|id| self.proposals.get(&id)).collect()
    }

    pub fn get_withdrawal(&self, id: String) -> Option<&Withdrawal> {
        self.withdrawals.get(&id)
    }

    pub fn get_claimable(&self, account_id: AccountId) -> U128 {
        U128(self.claimable.get(&account_id).copied().unwrap_or(0))
    }

    pub fn get_accounting(&self) -> Accounting {
        let (bonded, unbonding) = self
            .operators
            .values()
            .fold((0u128, 0u128), |(b, u), op| (b + op.bond.0, u + op.unbonding.0));
        let total = self.liquidity + self.escrowed + bonded + unbonding + self.insurance + self.challenge_bonds + self.claimable_total;
        Accounting {
            liquidity: U128(self.liquidity),
            reserved: U128(self.reserved),
            available: U128(self.liquidity - self.reserved),
            escrowed: U128(self.escrowed),
            bonded: U128(bonded),
            unbonding: U128(unbonding),
            insurance: U128(self.insurance),
            challenge_bonds: U128(self.challenge_bonds),
            claimable: U128(self.claimable_total),
            total: U128(total),
            in_flight: U128(self.in_flight()),
            max_in_flight: U128(self.max_in_flight()),
        }
    }

    /// Shape read by the ZEC SSL website (`server/src/sources/network.ts`).
    pub fn get_liquidity(&self) -> Vec<LiquidityView> {
        vec![LiquidityView {
            symbol: self.config.token_symbol.clone(),
            amount: format_units(self.liquidity - self.reserved, self.config.token_decimals),
            raw: U128(self.liquidity - self.reserved),
        }]
    }

    /// Shape read by the ZEC SSL website.
    pub fn get_total_bonded(&self) -> BondedView {
        let raw: u128 = self.operators.values().map(|op| op.slashable()).sum();
        BondedView { usd: raw as f64 / 10f64.powi(self.config.token_decimals as i32), raw: U128(raw) }
    }

    pub fn get_max_in_flight(&self) -> U128 {
        U128(self.max_in_flight())
    }
}

impl Contract {
    pub(crate) fn require_not_paused(&self) {
        require!(!self.paused, "contract is paused");
    }

    /// An operator who may approve: allowlisted and bonded at least `min_bond`.
    pub(crate) fn require_eligible_operator(&self) -> AccountId {
        let caller = env::predecessor_account_id();
        let op = self.operators.get(&caller).unwrap_or_else(|| env::panic_str("caller is not an operator"));
        require!(op.active, "operator is not active");
        require!(op.bond.0 >= self.config.min_bond.0, "operator bond is below the minimum");
        caller
    }

    pub(crate) fn is_eligible(&self, op: &Operator) -> bool {
        op.active && op.bond.0 >= self.config.min_bond.0
    }

    /// Value operators could take today: open direction-1 reservations (the ZEC side is
    /// held by operators) plus direction-2 escrow awaiting payout.
    pub(crate) fn in_flight(&self) -> u128 {
        self.reserved + self.escrowed
    }

    /// The combined slashable bonds of the `threshold` smallest eligible operators: the
    /// cheapest colluding quorum. Zero if fewer than `threshold` are eligible.
    pub(crate) fn max_in_flight(&self) -> u128 {
        let mut bonds: Vec<u128> = self.operators.values().filter(|op| self.is_eligible(op)).map(|op| op.slashable()).collect();
        let t = self.config.threshold as usize;
        if bonds.len() < t {
            return 0;
        }
        bonds.sort_unstable();
        bonds.iter().take(t).sum()
    }

    pub(crate) fn require_capacity(&mut self, amount: u128) {
        require!(amount > 0, "amount must be positive");
        require!(amount <= self.config.per_order_cap.0, "amount exceeds the per-order cap");
        require!(self.in_flight() + amount <= self.max_in_flight(), "amount would exceed the bonded capacity");
        let t = now();
        if t >= self.epoch_start + self.config.epoch_length_ns.0 {
            self.epoch_start = t;
            self.epoch_volume = 0;
        }
        require!(self.epoch_volume + amount <= self.config.epoch_cap.0, "amount exceeds the epoch cap");
        self.epoch_volume += amount;
    }

    pub(crate) fn add_operator_internal(&mut self, account_id: AccountId) {
        match self.operators.get_mut(&account_id) {
            Some(op) => op.active = true,
            None => {
                self.operators.insert(
                    account_id.clone(),
                    Operator {
                        account_id,
                        active: true,
                        bond: U128(0),
                        unbonding: U128(0),
                        unlock_at: U64(0),
                        slashed: U128(0),
                        equivocations: 0,
                    },
                );
            }
        }
    }

    /// Takes up to `amount` from an operator's bond, then its unbonding balance.
    /// Returns what was actually taken.
    pub(crate) fn slash_internal(&mut self, operator: &AccountId, amount: u128) -> u128 {
        let op = self.operators.get_mut(operator).unwrap_or_else(|| env::panic_str("unknown operator"));
        let from_bond = amount.min(op.bond.0);
        let from_unbonding = (amount - from_bond).min(op.unbonding.0);
        op.bond = U128(op.bond.0 - from_bond);
        op.unbonding = U128(op.unbonding.0 - from_unbonding);
        let taken = from_bond + from_unbonding;
        op.slashed = U128(op.slashed.0 + taken);
        events::slashed(operator, taken);
        taken
    }

    /// Slashes `total` split evenly across `operators`, capped by what each has.
    pub(crate) fn slash_evenly(&mut self, operators: &[AccountId], total: u128) -> u128 {
        if operators.is_empty() || total == 0 {
            return 0;
        }
        let share = total.div_ceil(operators.len() as u128);
        let mut collected = 0;
        for op in operators {
            let want = share.min(total - collected);
            collected += self.slash_internal(op, want);
            if collected >= total {
                break;
            }
        }
        collected
    }
}
