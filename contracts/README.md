# ZEC SSL NEAR contracts

```
contracts/
├── ssl/           the ZEC SSL contract: vault, bonds, orders, disputes, governance
├── test-token/    testnet-only NEP-141 "USDC" with a capped public faucet
└── scripts/       toolchain install, tests, testnet deployment
```

Implements Phase 3 of [docs/testnet-plan.md](../docs/testnet-plan.md), following its decisions:
- **Approvals:** operators approve on-chain from their own accounts, so every approval is attributable.
- **Layout:** one contract holds the vault, the bonds and the disputes.
- **Bonds:** in the vault token (USDC).
- **Token:** our own test token on testnet.
- **Governance:** an arbiter multisig resolves disputes.

## What it enforces

- **Nothing leaves the vault without `threshold` operator approvals of identical terms**, or an executed arbiter decision.
- **Equivocation is recorded, not reverted.** An operator who approved an order and then submits different terms for it gets an on-chain mark that arbiters can slash on.
- **Caps tied to bonds (FR-18).** Value in flight (open reservations plus direction-2 escrow) can never exceed the combined bonds of the `threshold` smallest eligible operators, the cheapest colluding quorum. There are also per-order and per-epoch caps.
- **Unbonding funds stay slashable** for `unbonding_delay_ns`, which the contract requires to exceed the release, dispute and challenge windows.
- **Direction-2 payouts stay shielded.** Escrow requires a unified address (`u1…`/`utest1…`); transparent addresses are refused.
- **Failed payouts become claimable**, so a recipient without token storage can't wedge an order.
- **Every token is accounted for.** `get_accounting().total` equals the contract's token balance. The API checks this live and shows it on the website's Treasury page.

## Lifecycle

**Direction 1** (shielded ZEC → USDC):
1. `approve_order` × t. At the threshold the order opens and its payout is reserved.
2. Then one of three paths:
   - `approve_release` × t pays the recipient.
   - `approve_refund` × t, after the ZEC was refunded on Zcash.
   - After the release deadline, the recipient disputes. They send a `{"action":"dispute"}` transfer carrying the challenge bond. Arbiters then resolve it: if upheld, the approvers are slashed and the recipient is paid from their bonds.
3. `close_expired` cleans up orders where nothing happened.

**Direction 2** (USDC → shielded ZEC):
1. The user locks USDC with `{"action":"lock_for_zec", ...}`.
2. Operators pay the ZEC, then `claim_settlement` × t with the Zcash txid.
3. A challenge window opens. During it, the user may `{"action":"challenge"}`.
4. If unchallenged, `finalize_settlement` moves the escrow into liquidity.
5. If operators never claim, the user can `reclaim` after the deadline.
6. If challenged, arbiters resolve it: if upheld, the escrow is refunded and the claimers are slashed.

**Governance** (`propose` / `approve_proposal`, arbiter t-of-n): add or remove operators, slash, resolve disputes, set config, pause, withdraw insurance.

**Views used by the website:** `get_liquidity`, `get_total_bonded`, `get_operators`, `get_orders`, `get_config`, `get_accounting`.

Every state change emits a NEP-297 event (`EVENT_JSON:{"standard":"zecssl",...}`).

## Build and test (Linux/WSL)

```bash
bash contracts/scripts/install-toolchain.sh   # Rust 1.95 (pinned), wasm target, cargo-near
bash contracts/scripts/test.sh                # unit tests, wasm builds, sandbox tests
```

The sandbox tests run the real wasm on a local NEAR node with real token transfers. After every step they check that the contract's token balance equals its accounted total.

## Deploy to testnet

`scripts/deploy-testnet.sh` deploys the token and the vault and registers the vault with the token. It signs with your NEAR CLI keychain, so you run it. Then set `NEAR_VAULT_CONTRACT` in `server/.env` and the website shows live operators, bonds, orders and the reserve check.

## Before mainnet

- External audit (plan Phase 8).
- Reproducible builds (`cargo near build reproducible-wasm`) so the deployed code hash can be verified.
- Storage: orders are kept forever and the contract pays their storage. Mainnet needs pruning or an operator-paid storage deposit.
- Mainnet parameters (bond size, caps, windows) are set after testnet load tests (PRD: open parameters).
