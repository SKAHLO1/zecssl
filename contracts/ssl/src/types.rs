use near_sdk::json_types::{U128, U64};
use near_sdk::{near, AccountId};

pub type OrderId = String;

/// Protocol parameters. Amounts are in the vault token's base units (USDC: 6 decimals);
/// durations are nanoseconds, matching `env::block_timestamp`.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// The NEP-141 token the vault pays out and operators bond in.
    pub token: AccountId,
    pub token_symbol: String,
    pub token_decimals: u8,
    /// Operator approvals needed to open, release, refund or settle an order.
    pub threshold: u8,
    /// Arbiter approvals needed to execute a governance proposal.
    pub arbiter_threshold: u8,
    /// Smallest bond that lets an operator approve anything.
    pub min_bond: U128,
    /// How long unbonded funds stay slashable before they can be withdrawn. Must exceed
    /// the whole release-plus-dispute period, so an operator cannot exit ahead of a slash.
    pub unbonding_delay_ns: U64,
    /// After an order's deposit deadline, how long operators have to release it.
    pub release_window_ns: U64,
    /// After the release deadline passes unreleased, how long the user has to dispute.
    pub dispute_window_ns: U64,
    /// Direction 2: how long a settlement claim stays open to challenge.
    pub challenge_window_ns: U64,
    /// What a disputer or challenger posts; lost if the dispute fails.
    pub challenge_bond: U128,
    /// Largest single order.
    pub per_order_cap: U128,
    /// Most volume opened per epoch.
    pub epoch_cap: U128,
    pub epoch_length_ns: U64,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct Operator {
    pub account_id: AccountId,
    /// Allowed to approve. Arbiters add and remove operators.
    pub active: bool,
    pub bond: U128,
    /// Requested for withdrawal; still slashable until `unlock_at`.
    pub unbonding: U128,
    pub unlock_at: U64,
    pub slashed: U128,
    /// Times this operator approved conflicting terms for one order.
    pub equivocations: u32,
}

impl Operator {
    pub fn slashable(&self) -> u128 {
        self.bond.0 + self.unbonding.0
    }
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Shielded ZEC in, vault token out on NEAR.
    ZecToNear,
    /// Vault token escrowed on NEAR, shielded ZEC out.
    NearToZec,
}

/// What operators agree to for a shielded-ZEC-in order. All approvers must submit
/// identical terms.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderTerms {
    pub recipient: AccountId,
    pub amount_out: U128,
    /// ZEC the user must deposit, in zatoshis.
    pub zec_amount_zat: U64,
    /// The deposit must confirm on Zcash by this time.
    pub deposit_deadline: U64,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OrderState {
    // Direction 1
    /// Some operators approved; not yet at threshold.
    Proposed,
    /// Threshold approved; liquidity reserved; awaiting the deposit and release.
    Open,
    Released,
    /// Deposit was refunded on Zcash; reservation released.
    Refunded,
    /// Nothing happened before the dispute window closed.
    Expired,
    Disputed,
    /// Dispute upheld: the recipient was paid from slashed bonds.
    Compensated,
    // Direction 2
    /// User's tokens escrowed; awaiting the ZEC payout.
    Locked,
    /// Operators claim the ZEC was paid; challenge window running.
    Claimed,
    Challenged,
    /// Claim stood; escrow moved to vault liquidity.
    Settled,
    /// Escrow returned to the user.
    Reimbursed,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct Order {
    pub id: OrderId,
    pub direction: Direction,
    pub state: OrderState,
    /// Direction 1: what operators approved. Direction 2: what the user locked for.
    pub terms: OrderTerms,
    /// Operators who approved opening (direction 1) or claimed settlement (direction 2).
    pub approvers: Vec<AccountId>,
    pub release_approvals: Vec<AccountId>,
    pub refund_approvals: Vec<AccountId>,
    pub created_at: U64,
    pub opened_at: U64,
    /// Direction 1: release must happen before this.
    pub release_deadline: U64,
    pub closed_at: U64,
    // Direction 2
    /// The account that locked tokens.
    pub user: Option<AccountId>,
    pub escrow: U128,
    /// Shielded unified address the ZEC goes to.
    pub zec_address: Option<String>,
    /// Zcash txid operators claim paid the user.
    pub zcash_txid: Option<String>,
    pub claimed_at: U64,
    /// Who disputed or challenged, and the bond they posted.
    pub challenger: Option<AccountId>,
    pub challenge_bond: U128,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GovAction {
    AddOperator { account_id: AccountId },
    RemoveOperator { account_id: AccountId },
    Slash { operator: AccountId, amount: U128, reason: String },
    /// Direction 1 dispute (deposit proven, no release) or direction 2 challenge
    /// (payment not proven). `upheld` means the user was right.
    ResolveDispute { order_id: OrderId, upheld: bool },
    SetConfig { config: Config },
    Pause,
    Unpause,
    /// Moves slashed funds (insurance) to a receiver, e.g. to cover a shortfall.
    WithdrawInsurance { receiver: AccountId, amount: U128 },
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    pub id: u64,
    pub action: GovAction,
    pub approvals: Vec<AccountId>,
    pub executed: bool,
    pub created_at: U64,
}

/// A t-of-n operator request to withdraw vault liquidity.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct Withdrawal {
    pub id: String,
    pub receiver: AccountId,
    pub amount: U128,
    pub approvals: Vec<AccountId>,
    pub executed: bool,
}

/// Every token the contract holds, by purpose. `total` must equal the contract's token
/// balance; this is what a reserve attestation checks.
#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq)]
pub struct Accounting {
    pub liquidity: U128,
    pub reserved: U128,
    pub available: U128,
    pub escrowed: U128,
    pub bonded: U128,
    pub unbonding: U128,
    pub insurance: U128,
    pub challenge_bonds: U128,
    pub claimable: U128,
    pub total: U128,
    pub in_flight: U128,
    pub max_in_flight: U128,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidityView {
    pub symbol: String,
    /// Human-readable decimal amount.
    pub amount: String,
    pub raw: U128,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq)]
pub struct BondedView {
    /// Total slashable bonds in the vault token, which is a USD stablecoin.
    pub usd: f64,
    pub raw: U128,
}

/// The JSON `msg` of an `ft_transfer_call` into the contract.
#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TransferMsg {
    /// Operator adds to its own bond.
    Bond,
    /// Anyone adds vault liquidity.
    Liquidity,
    /// Direction 1: recipient disputes an unreleased order; amount is the challenge bond.
    Dispute { order_id: OrderId },
    /// Direction 2: escrow tokens for a shielded ZEC payout.
    LockForZec { order_id: OrderId, zec_address: String, zec_amount_zat: U64, deadline: U64 },
    /// Direction 2: challenge a settlement claim; amount is the challenge bond.
    Challenge { order_id: OrderId },
}
