//! Order lifecycles.
//!
//! Direction 1 (shielded ZEC → token): operators approve identical terms; at threshold the
//! order opens and its payout is reserved. Operators then approve the release (after
//! each has seen the deposit on Zcash) or a refund (after refunding it on Zcash). If the
//! release deadline passes, the recipient may dispute.
//!
//! Direction 2 (token → shielded ZEC): the user escrows tokens; operators pay ZEC and
//! claim settlement with the Zcash txid; the claim can be challenged during a window;
//! unchallenged claims settle into liquidity.

use near_sdk::json_types::{U128, U64};
use near_sdk::{env, near, require, AccountId, Promise};

use crate::{events, now, validate_id, Contract, ContractExt, Direction, Order, OrderId, OrderState, OrderTerms};

fn new_order(id: OrderId, direction: Direction, state: OrderState, terms: OrderTerms) -> Order {
    Order {
        id,
        direction,
        state,
        terms,
        approvers: vec![],
        release_approvals: vec![],
        refund_approvals: vec![],
        created_at: U64(now()),
        opened_at: U64(0),
        release_deadline: U64(0),
        closed_at: U64(0),
        user: None,
        escrow: U128(0),
        zec_address: None,
        zcash_txid: None,
        claimed_at: U64(0),
        challenger: None,
        challenge_bond: U128(0),
    }
}

/// A shielded unified address: starts with u1 (mainnet) or utest1 (testnet). Transparent
/// and Sapling-only addresses are refused, so direction-2 payouts stay shielded.
pub(crate) fn is_unified_address(addr: &str) -> bool {
    (addr.starts_with("u1") || addr.starts_with("utest1"))
        && addr.len() >= 60
        && addr.len() <= 512
        && addr.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

impl Contract {
    pub(crate) fn order_mut(&mut self, order_id: &OrderId) -> Order {
        self.orders.get(order_id).cloned().unwrap_or_else(|| env::panic_str("unknown order"))
    }

    pub(crate) fn save_order(&mut self, order: Order) {
        if !self.orders.contains_key(&order.id) {
            self.order_ids.push(order.id.clone());
        }
        self.orders.insert(order.id.clone(), order);
    }

    /// Records `caller` in `list`, returning whether the threshold is now met. Returns
    /// `None` if the caller had already approved.
    fn add_approval(list: &mut Vec<AccountId>, caller: &AccountId, threshold: u8) -> Option<bool> {
        if list.contains(caller) {
            return None;
        }
        list.push(caller.clone());
        Some(list.len() >= threshold as usize)
    }

    pub(crate) fn release_reservation(&mut self, order: &Order) {
        self.reserved -= order.terms.amount_out.0;
    }
}

#[near]
impl Contract {
    // ---------- direction 1 ----------

    /// One operator's approval of an order's terms. The first approval creates the order;
    /// later approvals must match it exactly. An operator who approved and now submits
    /// different terms for the same order is recorded as equivocating (slashable by
    /// arbiters) instead of failing, so the evidence is kept on chain.
    pub fn approve_order(&mut self, order_id: OrderId, terms: OrderTerms) -> OrderState {
        self.require_not_paused();
        validate_id(&order_id);
        let caller = self.require_eligible_operator();

        let mut order = match self.orders.get(&order_id) {
            Some(o) => o.clone(),
            None => {
                require!(terms.amount_out.0 > 0, "amount must be positive");
                require!(terms.zec_amount_zat.0 > 0, "ZEC amount must be positive");
                require!(terms.deposit_deadline.0 > now(), "deposit deadline is in the past");
                new_order(order_id.clone(), Direction::ZecToNear, OrderState::Proposed, terms.clone())
            }
        };
        require!(order.direction == Direction::ZecToNear, "not a direction-1 order");

        if order.terms != terms {
            if order.approvers.contains(&caller) {
                let op = self.operators.get_mut(&caller).expect("eligible operator exists");
                op.equivocations += 1;
                events::equivocation(&order_id, &caller);
                return order.state;
            }
            env::panic_str("terms differ from the approved terms");
        }
        if order.state != OrderState::Proposed {
            return order.state;
        }

        let Some(reached) = Self::add_approval(&mut order.approvers, &caller, self.config.threshold) else {
            return order.state;
        };
        events::approval(&order_id, "open", &caller, order.approvers.len(), self.config.threshold);
        if reached {
            let amount = order.terms.amount_out.0;
            require!(self.liquidity - self.reserved >= amount, "not enough unreserved liquidity");
            self.require_capacity(amount);
            self.reserved += amount;
            order.state = OrderState::Open;
            order.opened_at = U64(now());
            order.release_deadline = U64(order.terms.deposit_deadline.0 + self.config.release_window_ns.0);
            events::order(&order_id, OrderState::Open, None);
        }
        let state = order.state;
        self.save_order(order);
        state
    }

    /// One operator's approval to pay an open order's recipient. Each operator must have
    /// seen the deposit on Zcash itself before calling this.
    pub fn approve_release(&mut self, order_id: OrderId) -> Option<Promise> {
        self.require_not_paused();
        let caller = self.require_eligible_operator();
        let mut order = self.order_mut(&order_id);
        require!(order.state == OrderState::Open, "order is not open");
        let Some(reached) = Self::add_approval(&mut order.release_approvals, &caller, self.config.threshold) else {
            return None;
        };
        events::approval(&order_id, "release", &caller, order.release_approvals.len(), self.config.threshold);
        if !reached {
            self.save_order(order);
            return None;
        }
        let amount = order.terms.amount_out.0;
        let recipient = order.terms.recipient.clone();
        self.release_reservation(&order);
        self.liquidity -= amount;
        order.state = OrderState::Released;
        order.closed_at = U64(now());
        events::order(&order_id, OrderState::Released, None);
        self.save_order(order);
        Some(self.payout(recipient, amount, &format!("zec-ssl order {order_id}")))
    }

    /// One operator's approval that the order's deposit was refunded on Zcash (underpaid,
    /// late, or the order was abandoned). Releases the reservation.
    pub fn approve_refund(&mut self, order_id: OrderId) -> OrderState {
        let caller = self.require_eligible_operator();
        let mut order = self.order_mut(&order_id);
        require!(order.state == OrderState::Open, "order is not open");
        let Some(reached) = Self::add_approval(&mut order.refund_approvals, &caller, self.config.threshold) else {
            return order.state;
        };
        events::approval(&order_id, "refund", &caller, order.refund_approvals.len(), self.config.threshold);
        if reached {
            self.release_reservation(&order);
            order.state = OrderState::Refunded;
            order.closed_at = U64(now());
            events::order(&order_id, OrderState::Refunded, None);
        }
        let state = order.state;
        self.save_order(order);
        state
    }

    /// Anyone may close an order nothing happened to: a proposal that never reached the
    /// threshold before its deposit deadline, or an open order whose dispute window has
    /// closed without a dispute.
    pub fn close_expired(&mut self, order_id: OrderId) -> OrderState {
        let mut order = self.order_mut(&order_id);
        let t = now();
        match order.state {
            OrderState::Proposed => require!(t > order.terms.deposit_deadline.0, "deposit deadline has not passed"),
            OrderState::Open => {
                require!(t > order.release_deadline.0 + self.config.dispute_window_ns.0, "dispute window has not closed");
                self.release_reservation(&order);
            }
            _ => env::panic_str("order cannot expire in its current state"),
        }
        order.state = OrderState::Expired;
        order.closed_at = U64(t);
        events::order(&order_id, OrderState::Expired, None);
        self.save_order(order);
        OrderState::Expired
    }

    // ---------- direction 2 ----------

    /// One operator's claim that the shielded ZEC was paid, naming the Zcash txid. At
    /// threshold the challenge window opens.
    pub fn claim_settlement(&mut self, order_id: OrderId, zcash_txid: String) -> OrderState {
        self.require_not_paused();
        let caller = self.require_eligible_operator();
        require!(zcash_txid.len() == 64 && zcash_txid.bytes().all(|b| b.is_ascii_hexdigit()), "txid must be 64 hex characters");
        let mut order = self.order_mut(&order_id);
        require!(order.direction == Direction::NearToZec, "not a direction-2 order");
        require!(order.state == OrderState::Locked, "order is not awaiting settlement");
        let txid = zcash_txid.to_ascii_lowercase();
        match &order.zcash_txid {
            Some(existing) if *existing != txid => {
                if order.approvers.contains(&caller) {
                    let op = self.operators.get_mut(&caller).expect("eligible operator exists");
                    op.equivocations += 1;
                    events::equivocation(&order_id, &caller);
                    return order.state;
                }
                env::panic_str("txid differs from earlier claims");
            }
            Some(_) => {}
            None => order.zcash_txid = Some(txid),
        }
        let Some(reached) = Self::add_approval(&mut order.approvers, &caller, self.config.threshold) else {
            return order.state;
        };
        events::approval(&order_id, "settle", &caller, order.approvers.len(), self.config.threshold);
        if reached {
            order.state = OrderState::Claimed;
            order.claimed_at = U64(now());
            events::order(&order_id, OrderState::Claimed, None);
        }
        let state = order.state;
        self.save_order(order);
        state
    }

    /// Anyone may settle an unchallenged claim once its window has closed; the escrow
    /// becomes vault liquidity.
    pub fn finalize_settlement(&mut self, order_id: OrderId) -> OrderState {
        let mut order = self.order_mut(&order_id);
        require!(order.state == OrderState::Claimed, "order has no open claim");
        require!(now() > order.claimed_at.0 + self.config.challenge_window_ns.0, "challenge window has not closed");
        self.escrowed -= order.escrow.0;
        self.liquidity += order.escrow.0;
        order.state = OrderState::Settled;
        order.closed_at = U64(now());
        events::order(&order_id, OrderState::Settled, None);
        self.save_order(order);
        OrderState::Settled
    }

    /// Returns a direction-2 escrow to its user when operators never claimed settlement
    /// within the deadline plus release window. Anyone may trigger it; funds go to the user.
    pub fn reclaim(&mut self, order_id: OrderId) -> Promise {
        let mut order = self.order_mut(&order_id);
        require!(order.state == OrderState::Locked, "order is not awaiting settlement");
        require!(now() > order.terms.deposit_deadline.0 + self.config.release_window_ns.0, "settlement deadline has not passed");
        let user = order.user.clone().expect("direction-2 orders have a user");
        let amount = order.escrow.0;
        self.escrowed -= amount;
        order.state = OrderState::Reimbursed;
        order.closed_at = U64(now());
        events::order(&order_id, OrderState::Reimbursed, None);
        self.save_order(order);
        self.payout(user, amount, &format!("zec-ssl reclaim {order_id}"))
    }
}

impl Contract {
    /// `ft_on_transfer` action: the recipient of an unreleased direction-1 order disputes it.
    pub(crate) fn open_dispute(&mut self, sender: AccountId, order_id: OrderId, amount: u128) -> u128 {
        let mut order = self.order_mut(&order_id);
        require!(order.direction == Direction::ZecToNear, "not a direction-1 order");
        require!(order.state == OrderState::Open, "order is not open");
        require!(sender == order.terms.recipient, "only the recipient can dispute");
        let t = now();
        require!(t > order.release_deadline.0, "release deadline has not passed");
        require!(t <= order.release_deadline.0 + self.config.dispute_window_ns.0, "dispute window has closed");
        let bond = self.config.challenge_bond.0;
        require!(amount >= bond, "attached amount is below the challenge bond");
        self.challenge_bonds += bond;
        order.state = OrderState::Disputed;
        order.challenger = Some(sender.clone());
        order.challenge_bond = U128(bond);
        events::order(&order_id, OrderState::Disputed, Some(&sender));
        self.save_order(order);
        amount - bond
    }

    /// `ft_on_transfer` action: a user escrows tokens for a shielded ZEC payout.
    pub(crate) fn lock_for_zec(&mut self, user: AccountId, order_id: OrderId, zec_address: String, zec_amount_zat: U64, deadline: U64, amount: u128) {
        self.require_not_paused();
        validate_id(&order_id);
        require!(!self.orders.contains_key(&order_id), "order id already used");
        require!(is_unified_address(&zec_address), "payout address must be a shielded unified address (u1… or utest1…)");
        require!(zec_amount_zat.0 > 0, "ZEC amount must be positive");
        require!(deadline.0 > now(), "deadline is in the past");
        self.require_capacity(amount);
        self.escrowed += amount;
        let terms = OrderTerms { recipient: user.clone(), amount_out: U128(amount), zec_amount_zat, deposit_deadline: deadline };
        let mut order = new_order(order_id.clone(), Direction::NearToZec, OrderState::Locked, terms);
        order.user = Some(user.clone());
        order.escrow = U128(amount);
        order.zec_address = Some(zec_address);
        order.opened_at = U64(now());
        events::order(&order_id, OrderState::Locked, Some(&user));
        self.save_order(order);
    }

    /// `ft_on_transfer` action: the user challenges a settlement claim.
    pub(crate) fn challenge(&mut self, sender: AccountId, order_id: OrderId, amount: u128) -> u128 {
        let mut order = self.order_mut(&order_id);
        require!(order.state == OrderState::Claimed, "order has no open claim");
        require!(Some(&sender) == order.user.as_ref(), "only the user can challenge");
        require!(now() <= order.claimed_at.0 + self.config.challenge_window_ns.0, "challenge window has closed");
        let bond = self.config.challenge_bond.0;
        require!(amount >= bond, "attached amount is below the challenge bond");
        self.challenge_bonds += bond;
        order.state = OrderState::Challenged;
        order.challenger = Some(sender.clone());
        order.challenge_bond = U128(bond);
        events::order(&order_id, OrderState::Challenged, Some(&sender));
        self.save_order(order);
        amount - bond
    }

    /// Executes an arbiter decision on a dispute or challenge. Returns the payouts to make.
    pub(crate) fn resolve_dispute(&mut self, order_id: &OrderId, upheld: bool) -> Vec<(AccountId, u128, String)> {
        let mut order = self.order_mut(order_id);
        let challenger = order.challenger.clone().expect("disputed orders have a challenger");
        let bond = order.challenge_bond.0;
        self.challenge_bonds -= bond;
        let mut payouts = vec![];
        match (order.state, upheld) {
            // Deposit proven, never released: pay the recipient from the approvers' bonds.
            (OrderState::Disputed, true) => {
                let amount = order.terms.amount_out.0;
                let collected = self.slash_evenly(&order.approvers.clone(), amount);
                let from_insurance = (amount - collected.min(amount)).min(self.insurance);
                self.insurance -= from_insurance;
                self.insurance += collected.saturating_sub(amount);
                let paid = collected.min(amount) + from_insurance;
                self.release_reservation(&order);
                order.state = OrderState::Compensated;
                payouts.push((order.terms.recipient.clone(), paid, format!("zec-ssl compensation {order_id}")));
                payouts.push((challenger, bond, format!("zec-ssl dispute bond {order_id}")));
            }
            (OrderState::Disputed, false) => {
                self.insurance += bond;
                self.release_reservation(&order);
                order.state = OrderState::Expired;
            }
            // Payment not proven: refund the escrow and slash the claimers by its value.
            (OrderState::Challenged, true) => {
                let amount = order.escrow.0;
                self.escrowed -= amount;
                self.insurance += self.slash_evenly(&order.approvers.clone(), amount);
                order.state = OrderState::Reimbursed;
                payouts.push((order.user.clone().expect("direction-2 orders have a user"), amount, format!("zec-ssl refund {order_id}")));
                payouts.push((challenger, bond, format!("zec-ssl challenge bond {order_id}")));
            }
            (OrderState::Challenged, false) => {
                self.insurance += bond;
                self.escrowed -= order.escrow.0;
                self.liquidity += order.escrow.0;
                order.state = OrderState::Settled;
            }
            _ => env::panic_str("order is not disputed or challenged"),
        }
        order.closed_at = U64(now());
        events::order(order_id, order.state, None);
        self.save_order(order);
        payouts
    }
}
