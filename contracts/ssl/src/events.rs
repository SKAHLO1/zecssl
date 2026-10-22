//! NEP-297 events. The ZEC SSL API indexes these into the activity feed.

use near_sdk::serde_json::{json, Value};
use near_sdk::{env, AccountId};

use crate::{OrderId, OrderState};

fn emit(event: &str, data: Value) {
    let log = json!({ "standard": "zecssl", "version": "1.0.0", "event": event, "data": [data] });
    env::log_str(&format!("EVENT_JSON:{log}"));
}

pub fn order(order_id: &OrderId, state: OrderState, by: Option<&AccountId>) {
    emit("order", json!({ "order_id": order_id, "state": state, "by": by }));
}

pub fn approval(order_id: &OrderId, kind: &str, operator: &AccountId, count: usize, threshold: u8) {
    emit("approval", json!({ "order_id": order_id, "kind": kind, "operator": operator, "count": count, "threshold": threshold }));
}

pub fn equivocation(order_id: &OrderId, operator: &AccountId) {
    emit("equivocation", json!({ "order_id": order_id, "operator": operator }));
}

pub fn bond(operator: &AccountId, kind: &str, amount: u128) {
    emit("bond", json!({ "operator": operator, "kind": kind, "amount": amount.to_string() }));
}

pub fn slashed(operator: &AccountId, amount: u128) {
    emit("slash", json!({ "operator": operator, "amount": amount.to_string() }));
}

pub fn liquidity(kind: &str, account: &AccountId, amount: u128) {
    emit("liquidity", json!({ "kind": kind, "account": account, "amount": amount.to_string() }));
}

pub fn payout(receiver: &AccountId, amount: u128, ok: bool) {
    emit("payout", json!({ "receiver": receiver, "amount": amount.to_string(), "ok": ok }));
}

pub fn governance(proposal_id: u64, kind: &str, by: &AccountId) {
    emit("governance", json!({ "proposal_id": proposal_id, "kind": kind, "by": by }));
}
