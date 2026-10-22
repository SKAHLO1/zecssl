# ZEC SSL — plan to a production testnet

Goal: every panel on the website is backed by a real, running system on Zcash testnet and NEAR testnet, operated like production (separate operators, monitoring, runbooks, caps), with both swap directions and every failure path working.

Time limits from the PRD are set aside. The phases below are ordered by dependency, and each ends with an exit test.

## Where we are

Done: the website (`web/`) and the public read API (`server/`). They already show live market, Zcash, NEAR and NEAR Intents data. They also define the interfaces the protocol must serve (see `README.md` → *Contract for ZEC SSL services*).

Missing: everything that actually moves funds.

| Website feature | Powered by | Status |
| --- | --- | --- |
| Swap: quote, deposit address, live order status | Coordinator order API + operator approvals | Not built |
| Swap: settlement on NEAR | NEAR contract `release` with t-of-n approvals | Not built |
| Swap: refund to shielded address | Coordinator refund flow + FROST-signed Zcash spend | Not built |
| Operator network: operators, health, bonds | Operator nodes `/health` + contract bonds | Not built |
| Operator network: signing rounds, events | Coordinator + `frostd` | Not built |
| Treasury: shielded balance, scan height | Coordinator view-only wallet (UFVK) | Not built |
| Treasury: vault liquidity, total bonded | NEAR contract view methods | Not built |
| Zcash panels on testnet | Our own Zebra + Zaino testnet nodes | Not deployed (today they show mainnet data from public APIs) |
| USDC → shielded ZEC (direction 2) | Contract escrow + challenge window | Not built |

## Target architecture

```
                         ┌───────────────── public ─────────────────┐
  Browser ── web (Next.js) ── read API (server/, TS) ──┬── CoinGecko, NEAR RPC
                                                       ├── Zebra/Zaino (testnet)
                                                       └── coordinator (read)
  Zcash wallet (Zashi, Zkool…) ── shielded ZEC ──► treasury address (Ironwood)

                         ┌───────────────── internal ───────────────┐
  coordinator (Rust) ── Postgres
     │  builds PCZTs, runs signing rounds via frostd, scans with the UFVK
     ├── frostd (ZF coordination server)
     ├── operator-1 (Rust) ── own Zebra+Zaino ── key share ── NEAR account
     ├── operator-2 (Rust) ── own Zebra+Zaino ── key share ── NEAR account
     └── operator-3 (Rust) ── own Zebra+Zaino ── key share ── NEAR account
                                   │ approvals, bonds
                                   ▼
                     NEAR contract  ssl.<org>.testnet  (vault + bonds + disputes)
```

Principles carried over from the PRD:

- The coordinator holds no key share. Operators never trust it: each one re-derives every check from its own chain view.
- Operators approve on NEAR with their own accounts, so every approval is attributable and slashable.
- No treasury transaction may have a transparent input or output. Each operator checks this before signing.

## Decisions to make first

These block the phases noted. Each has a recommendation.

| Decision | Options | Recommendation | Blocks |
| --- | --- | --- | --- |
| How operators authorize NEAR releases | (a) each operator sends an on-chain approval; (b) t-of-n ed25519 signatures verified in the contract; (c) one FROST-Ed25519 aggregate signature | **(a)**. It is the simplest to build, and on-chain approvals identify who approved what, which slashing needs. (c) hides which operators signed. | 3 |
| Contract layout | Separate vault, bonds and disputes contracts, or one contract | **One contract** with modules. A slash pays the user from bonds in the same call, with no cross-contract callbacks to get wrong. | 3 |
| Bond asset (PRD open question) | NEAR, USDC, ZEC on NEAR | **USDC**. Bond value then doesn't drift against the USDC it secures, so "bonds > value at risk" holds without an oracle. | 3 |
| Testnet USDC | Circle's NEAR testnet USDC (if available), or our own NEP-141 test token | **Our own test token** (`usdc.<org>.testnet`) with a faucet page. Supply and liquidity are then under our control. | 3, 5 |
| Dispute arbiters (PRD open question) | Core team multisig, independent committee | **2-of-3 multisig of named people** for testnet. Its powers stay limited to resolving disputes; it cannot move vault funds. | 6 |
| Confirmation depth (PRD open question) | Blocks vs wall-clock | **Wall-clock: 5 minutes** (12 blocks at 25 s), configurable. | 4 |
| Who runs the 3 operators | Team machines, partners | **Three different people on three different cloud accounts**, even on testnet. Otherwise the "one operator offline" test proves nothing. | 2, 9 |
| Hosting | Fly.io, Render, Railway, VMs + Terraform | **VMs + Terraform** for nodes and operators (stable disks for chain data); any host for web + read API. | 1, 9 |

## Phases

### Phase 1 — Chain infrastructure and dev harness

1. Run **Zebra on testnet** with **Zaino** as the indexer. The coordinator and each operator need their own pair.
2. Local **regtest harness**: Zebra regtest with NU6.3 (Ironwood) and NU7 activated, NEAR sandbox (`near-workspaces`), and one `docker compose up` that starts the coordinator, `frostd` and 3 operators. Every later phase tests against this first.
3. Monorepo layout:
   ```
   contracts/ssl/        near-sdk contract
   crates/ssl-types/     order, approval and event types shared by Rust services
   crates/ssl-policy/    the checks operators run before signing (pure, heavily tested)
   coordinator/          Rust service
   operator/             Rust service
   server/  web/         existing
   infra/                docker-compose, terraform, dashboards
   ```
4. CI: `cargo test`, `clippy`, contract tests in the NEAR sandbox, web typecheck and build, plus the end-to-end harness on every PR to `main`.
5. **Pin versions.** NU7 changes are landing in `librustzcash`, `orchard`, `pczt` and Zebra. Pin exact versions and upgrade deliberately.

> Verified (Phase 2 spike): Ironwood is NU6.3, live on mainnet since July 28, 2026. `pczt ≥ 0.8` and `orchard ≥ 0.15` support Ironwood spends with external signatures. Still open: whether Zebra regtest can activate NU6.3; until then the spike runs against testnet through `testnet.zec.rocks`. See [protocol/README.md](../protocol/README.md) for results.

**Exit:** the harness starts from scratch with one command, and the website's read API reports the regtest Zebra as its Zcash source.

### Phase 2 — Threshold keys and the shielded treasury (highest risk, start first)

The PRD flags wiring rerandomized FROST into the PCZT signing flow as the riskiest part. Prove it before building around it.

1. **DKG** with ZF's `frost-client`/`frostd` over the RedPallas ciphersuite (`reddsa`, rerandomized FROST). Output: each operator's spend-auth key share, plus the group verifying key `ak`.
2. **Viewing key:** generate `nk` and `rivk` and combine them with `ak` into the treasury's unified full viewing key (UFVK). `nk` and `rivk` grant viewing, not spending, and every operator gets them.
3. **Addresses:** one diversified address per order (diversifier index = order sequence number).
4. **Signing spike:** build a PCZT that spends a treasury note to an external shielded address. Prove it with the UFVK, run a 2-of-3 rerandomized FROST round using the action's randomizer, apply the spend-auth signature, extract and broadcast. Use ZF's demos and Zkool's implementation as references.
5. **Key share storage:** encrypt shares at rest (age or AES-GCM, key from the host's secret manager). Never persist FROST nonces.

**Exit:** on testnet, fund a treasury address from a normal wallet and spend from it with operators 1+2, then 2+3, then 1+3. Every resulting transaction has zero transparent inputs and outputs on an explorer.

> **Status (2026-10-22): exit test passed.** All three pairs spent from an Ironwood treasury on testnet with zero transparent I/O (blocks 4,529,462 / 470 / 480). See [protocol/README.md](../protocol/README.md).
>
> Not done yet in this phase:
> - Key shares are still stored in plaintext `frost-client` configs; item 5 (encrypted storage) remains.
> - All three operators ran on one machine.
> - Signing used upstream CLI tools driven by scripts, not the coordinator and operator services from Phase 4.

### Phase 3 — NEAR contract (can run in parallel with Phase 2)

One contract, `ssl.<org>.testnet`, in `near-sdk` (Rust).

**State:** operator set and threshold, bonds, orders, disputes, caps, pause flag, arbiter set.

**Methods:**

| Method | Who | Does |
| --- | --- | --- |
| `register_operator`, bond via `ft_transfer_call` | operator | Joins the set once its bond is at least the minimum |
| `request_unbond`, `withdraw_unbonded` | operator | Unbonding delay longer than the dispute window |
| `approve_order(order_id, recipient, amount_out, expiry)` | operator | Records the order once t operators approve with identical terms. Conflicting terms from the same operator = equivocation, which is slashable. |
| `approve_release(order_id)` | operator | Pays the recipient from vault liquidity once t operators approve, before the release deadline |
| `open_dispute(order_id)` + challenger bond | user | After the release deadline passes with no release |
| `resolve_dispute(order_id, upheld)` | arbiters | If upheld: slash the approvers' bonds, pay the user, return the challenger bond. If not: forfeit the challenger bond. |
| `slash(operator, amount, reason)` | arbiters | Manual slash (PRD MVP) |
| `deposit_liquidity` / `withdraw_liquidity` | operators (t-of-n) | Manage vault USDC |
| `pause` / `unpause` | arbiters | Emergency stop |
| Direction 2: `lock_for_zec`, `claim_settlement`, `challenge` | user / operators | Escrow, optimistic claim, challenge window |
| Views: `get_liquidity`, `get_total_bonded`, `get_order`, `get_operators`, `get_config` | anyone | The first two are already read by the website |

**Caps (FR-18, enabled from day one):** per-order and per-epoch limits. In-flight value must never exceed the combined bonds of the t smallest bonders.

**Events:** NEP-297 JSON logs for every state change. The read API and an indexer turn these into the activity feed.

**Exit:** sandbox tests cover every method, every revert, the equivocation path and caps. Deployed to testnet with three bonded operator accounts.

> **Status (2026-10-22): built and tested; not yet deployed.** See [contracts/README.md](../contracts/README.md).
> - 19 unit tests and 5 sandbox tests on the real wasm pass. The sandbox tests check after every step that the contract's token balance equals its accounted total.
> - The API and website read the contract's operators, bonds, orders and a live reserve check.
> - Remaining: run `contracts/scripts/deploy-testnet.sh` with your NEAR testnet accounts, then bond three operators.

### Phase 4 — Coordinator and operator nodes, direction 1

**Coordinator** (Rust, Postgres):

- **Order API:**
  - `POST /v1/orders` validates the request, prices it and proposes the order to operators.
  - The order only opens once t operators have approved on NEAR. Then it returns the order ID, deposit address, memo and expiry.
  - Quotes on testnet: mainnet ZEC/USD as reference price, minus the spread. Testnet ZEC has no market.
- **Scanner:** a view-only wallet on the UFVK (`zcash_client_backend` + `zcash_client_sqlite` against Zaino). It matches deposits by address and memo order ID and waits for the confirmation depth.
- **Settlement:** after operators confirm the deposit, asks them for `approve_release` and tracks the NEAR transaction.
- **Order state machine:** `quoted → awaiting_deposit → deposit_detected → confirming → signing → released`, plus `refunded`, `expired`, `disputed`. Every transition is stored as an event.
- **Read endpoints:** `/v1/orders`, `/v1/signing-rounds`, `/v1/events`, `/v1/treasury`, already consumed by `server/`.

**Operator node** (Rust):

- Its own Zebra + Zaino and its own UFVK scanner. Deposits are confirmed independently, never taken from the coordinator.
- **Policy engine** (`crates/ssl-policy`), run before every signature or approval:
  1. The order is approved on NEAR with matching terms and is not expired.
  2. The deposit is seen on its own chain view at the confirmation depth, and the amount covers the quote.
  3. For any Zcash spend: the PCZT's outputs match the approved order or refund exactly, the fee is ZIP-317-correct, and there are **no transparent inputs or outputs**.
  4. Caps are not exceeded.
- Signs FROST rounds through `frostd` and sends NEAR approvals from its own account.
- `GET /health` in the shape the website expects, plus Prometheus metrics.

**Exit:** a full direction-1 swap on testnet, driven only from the website and a normal Zcash wallet. It also settles with any one operator stopped.

> **Status (2026-10-22): direction 1 runs end to end on testnet.** See [protocol/README.md](../protocol/README.md).
> - Order `zs-1` was opened from the website and approved on the vault by all three operators, each with its own NEAR key.
> - 0.01 ZEC arrived from a separate user wallet. Operators approved the release after 10 confirmations, and the vault paid 11.994139 test USDC to the recipient, about 8 minutes end to end.
> - Remaining in this phase: re-run with one operator stopped; refunds (Phase 6); operators on separate hosts with their own Zebra and Zaino (Phase 8).

### Phase 5 — Website and read API on testnet

- **Network switch:** `NETWORK=testnet` selects the NEAR testnet RPC, our Zebra testnet node and testnet explorers. Panels that can only show mainnet data are labelled as mainnet context.
- **Swap destinations:** test USDC on NEAR testnet only. Quotes come from the coordinator, not NEAR Intents (Intents is mainnet-only and v1 scope).
- **Address checks:** refund addresses must start with `utest1`; recipients must be `.testnet` NEAR accounts.
- **Deposit step:** QR code and ZIP-321 payment URI (address, amount, memo), so wallets fill it in automatically.
- **Order page:** `/app/orders/[id]` with the full trail (approvals, deposit, confirmations, signing rounds, NEAR transaction) and a shareable link.
- **NEAR wallet connection** (NEAR Wallet Selector). Needed for opening disputes and for direction 2.
- **Faucet page:** links to a testnet ZEC faucet and mints test USDC.
- **Read API:** indexes contract events (NEP-297) for activity, bonds and liquidity history.

**Exit:** no panel shows "not connected" while all services are up. Each panel's "not connected" state still works when its service is stopped.

### Phase 6 — Failure paths

| Case | Behaviour | Test |
| --- | --- | --- |
| Underpaid or late deposit | FROST-signed refund to the user's shielded refund address (FR-10) | Send 90% of the quote; send after expiry |
| Order never funded | Expires; nothing to refund | Let it lapse |
| Operators miss the release deadline | User opens a dispute; arbiters check the payment disclosure; bonds pay the user | Stop 2 operators after the deposit is detected |
| Equivocation | Contract rejects it and flags the operator for slashing | Operator sends conflicting `approve_order` |
| Malicious coordinator | Operators refuse PCZTs paying anyone else | Coordinator builds a PCZT with a swapped output |
| Fewer than t online | Orders stop opening; nothing moves; resumes when quorum returns | Stop 2 of 3 |
| Reorg after detection | Release waits for the wall-clock depth | Regtest reorg in the harness |
| Coordinator crash mid-round | Round restarts with fresh nonces; no double spend | Kill coordinator during signing |

**Exit:** each row is an automated test in the harness, and the core rows also pass on testnet.

### Phase 7 — Direction 2: USDC → shielded ZEC

User locks USDC with `lock_for_zec`, naming a shielded address. The coordinator builds the payout PCZT with the order ID in the memo, and operators verify and sign 2-of-3. Operators then post `claim_settlement`, which opens the challenge window. With no challenge, the USDC moves to liquidity. If challenged, operators must produce a payment disclosure or the user is refunded from bonds.

**Exit:** a round trip of ZEC → USDC → ZEC on testnet, plus a challenged settlement that resolves correctly in each direction.

### Phase 8 — Production operations on testnet

- **Separate operators:** three people, three cloud accounts, three regions. Each operator holds their own share and their own NEAR key; no one has access to another's host.
- **Infrastructure as code:** Terraform for nodes and operators; one deploy pipeline per service; staged rollouts (regtest → testnet canary → testnet).
- **Monitoring and alerting:**
  - operator offline
  - signing round failed
  - scanner lag
  - release deadline approaching
  - caps near limit
  - treasury balance vs open liabilities mismatch
- **Public status page:** the website's `/app/status`, extended with history.
- **Runbooks:** operator onboarding, key share backup and recovery, pausing, dispute handling, coordinator failover.
- **Reserve attestations (FR-17):** periodic signed statement of treasury balance vs liabilities, published and shown on the Treasury page.
- **Security review:** internal review of `ssl-policy`, the contract and nonce handling. A public bug bounty for testnet, then an external audit as the gate to mainnet beta.

**Exit — testnet launch criteria:**

1. 100 consecutive swaps in both directions without manual intervention.
2. Zero transparent inputs or outputs across all treasury transactions, verified by a script against the chain.
3. Settles with any single operator offline.
4. Every failure-path test in Phase 6 passes.
5. Alerts fire in a game-day drill.

### Phase 9 — v1 on testnet (after launch)

From the PRD:

- NEAR Intents solver integration, so any Intents asset routes through ZEC SSL (needs Intents testnet support, or this moves to mainnet).
- 3-of-5 operators, with key resharing and treasury sweeps when the set changes.
- Batched payouts and randomized timing.
- Payment-disclosure dispute tooling.
- TypeScript and Flutter SDKs.

## Order of work and parallel tracks

```
Phase 1 infra/harness ──┬──► Phase 2 keys + signing spike ──┐
                        └──► Phase 3 contract ──────────────┼──► Phase 4 coordinator + operators ──► Phase 6 failure paths ──► Phase 7 direction 2 ──► Phase 8 launch
Phase 5 website (network switch, address checks, faucet, order page) ─── can start now; final wiring after Phase 4
```

Three tracks can run at once: **keys/signing** (Phase 2), **contract** (Phase 3) and **website** (Phase 5). The coordinator and operator work starts once Phases 2 and 3 have passed their exit tests.

## Risks

| Risk | Mitigation |
| --- | --- |
| Ironwood/NU7 crate support is incomplete or still changing | Phase 2 spike first; pin versions; track the ZF and Zodl releases |
| NEAR contracts can't verify shielded payments, so disputes rely on arbiters | Narrow arbiter powers, public dispute log, research a light verifier later |
| Operator collusion on testnet shows nothing because the team runs all three | Independent operators from Phase 8; publish who they are |
| Correlating deposits and releases by amount and timing | Batching and randomized delays in v1; show a privacy note in the UI until then |
| Public data APIs rate-limit the read API | Own Zebra/Zaino for all Zcash data on testnet |
