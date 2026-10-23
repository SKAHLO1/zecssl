# ZEC SSL protocol

Rust code that touches keys and chains: the coordinator and operator services (Phase 4), their libraries, and the Phase 2 FROST signing spike from [docs/testnet-plan.md](../docs/testnet-plan.md).

```
protocol/
├── ssl-node/          the services: ssl-coordinator and ssl-operator
├── ssl-policy/        order types and the checks operators run before approving (pure, tested)
├── ssl-wallet/        view-only treasury wallet: lightwalletd sync, per-order addresses, deposits
├── ssl-near/          minimal NEAR client: view calls and signed function calls
├── ssl-sign/          prepare a PCZT for external signing, apply and verify signatures, extract the final tx
├── run/               run the services locally against testnet; a throwaway user wallet
├── spike/             Phase 2: frostd, DKG, treasury wallet, 2-of-3 spends
└── scripts/           build the pinned upstream tools; build and test ssl-sign
```

## Services (direction 1: shielded ZEC → USDC on NEAR)

**Coordinator** (`ssl-coordinator`). It holds no key share and no NEAR key, only the treasury's viewing key.
- Quotes from its own price feed.
- Gives each order a fresh shielded deposit address (diversifier index 1000 + order number).
- Proposes the order to every operator, and returns the deposit address **only once the vault reports the order open**.
- Tracks deposits and order state, nudges operators once a deposit is paid, and records events.
- **Refunds** an underpaid deposit (after the deadline) or a late one: it builds the refund (the deposit minus `REFUND_FEE_ZAT`, to the refund address, with change back to the treasury), proves it, and runs a two-round rerandomized FROST round with the first threshold of operators to commit. It aggregates the shares, checks the extracted transaction is fully shielded, and broadcasts it. A failed round is retried after two minutes with the same transaction.
- Serves the API the website uses: `/v1/quote`, `/v1/orders`, `/v1/orders/{id}`, `/v1/events`, `/v1/treasury`, `/v1/signing-rounds`, `/health`.

**Operator** (`ssl-operator`). Each runs its own NEAR key, its own wallet and its own price feed, and never trusts the coordinator.
- **Approving an order:** it approves on the vault only if [`ssl-policy`](ssl-policy/src/lib.rs) passes:
  - the deposit address is its *own* derivation for the stated index;
  - the payout doesn't exceed its own ZEC price (after spread, plus tolerance);
  - the order is within its size and deadline bounds;
  - the recipient is a valid NEAR account.
- **Approving a release:** it approves only when its *own* wallet shows the deposit paid in full, at the required depth, and first mined before the deadline. Underpaid or late deposits are never released.
- **Signing a refund** (`/v1/signing/round1`, `/v1/signing/round2`): before committing nonces, it checks with its own data that:
  - the order is still open on the vault, and its own wallet shows it underpaid after the deadline, or late;
  - [`check_refund`](ssl-policy/src/lib.rs) passes: no transparent or Sapling parts, nothing redacted, every output goes to the refund address it stored when it approved the order or back to the treasury, the refund is the deposit minus at most `MAX_REFUND_FEE_ZAT`, and the fee is within that allowance;
  - the transaction expires within 100 blocks;
  - it hasn't signed a *different* refund for the order that could still be mined.

  In round 2 it signs only the sighash it computed itself, with its own randomizer. Nonces live only in memory, are used once, and expire after 10 minutes. Once its own wallet sees the refund mined at the required depth, it approves the refund on the vault.
- **Proposals** must carry a shielded refund address that the operator can decode; it is stored with the order.
- **Health:** `/health` reports its NEAR account, its bond from the contract, signature count, wallet heights and last error. The website shows these on the Operator network page.

The coordinator authenticates to operators with a shared bearer token. That's not what keeps them safe, though: operators re-check every proposal against their own rules.

### Run locally against testnet

```bash
cd protocol && CARGO_TARGET_DIR=~/zec-ssl-deps/target-protocol cargo build --release -p ssl-node
VAULT=<prefix>-vault.testnet PREFIX=<prefix> bash protocol/run/start-local.sh
```

This starts three operators (`:7101`–`:7103`, signing as `<prefix>-op1..3.testnet` from your NEAR CLI keychain) and the coordinator (`:7000`). Each operator loads its own FROST share from the Phase 2 DKG (`~/zec-ssl-spike/operators/opN.toml`); the coordinator loads only the public key package. Each has its own state under `~/zec-ssl-run`. Then set these in `server/.env`:

```
COORDINATOR_URL=http://127.0.0.1:7000
OPERATOR_ENDPOINTS=<prefix>-op1.testnet=http://127.0.0.1:7101,<prefix>-op2.testnet=http://127.0.0.1:7102,<prefix>-op3.testnet=http://127.0.0.1:7103
```

The website's swap then quotes from the coordinator, pays out in the vault's token, and follows each order live.

`run/user-wallet.sh` is a throwaway testnet wallet that plays the user and deposits into orders.

### Not in this phase

- **Direction 2** (USDC → shielded ZEC) is Phase 7.
- **Independence:** all services share one machine and one lightwalletd here. Production operators run on separate hosts with their own Zebra and Zaino (Phase 8).

## Build (Linux or WSL)

Requires Rust ≥ 1.89, `openssl`, `python3`, `xxd`.

```bash
bash protocol/scripts/build-deps.sh     # frost-tools + zcash-devtool, pinned, into ~/zec-ssl-deps/bin
bash protocol/scripts/test-ssl-sign.sh  # tests ssl-sign and installs it next to them
```

Upstream tools, pinned by commit in `build-deps.sh`:

| Tool | From | Used for |
| --- | --- | --- |
| `frostd` | ZcashFoundation/frost-tools `53df99c` | FROST coordination server (audited by Least Authority) |
| `frost-client` | same | DKG; coordinator and participant roles in signing |
| `zcash-sign generate` | same | Treasury UFVK from the FROST group key |
| `zcash-devtool` | zcash/zcash-devtool `0b3436b` | View-only wallet, PCZT create/prove/send via `testnet.zec.rocks` |

`ssl-sign` pins the exact librustzcash revision devtool builds against, so PCZTs round-trip between them.

## Run the spike

All state (key shares, viewing key, wallet, PCZTs) goes to `~/zec-ssl-spike`, never into the repository.

```bash
cd protocol/spike
bash 02-dkg.sh                 # 3 operators, 2-of-3 RedPallas DKG through a local frostd
bash 05-frost-selftest.sh      # offline: every operator pair signs; signatures verified
bash 03-treasury.sh            # UFVK + address, view-only wallet, sync, balance
# fund the printed address with testnet ZEC, wait for confirmations, then:
bash 04-spend.sh op1 op2 <utest1… address> 100000
bash 04-spend.sh op2 op3 <utest1… address> 100000
bash 04-spend.sh op1 op3 <utest1… address> 100000
# or all three pairs in turn, waiting for confirmations between them:
bash 06-all-pairs.sh 1000000
bash 07-verify.sh              # re-check every broadcast spend
```

`04-spend.sh` refuses any transaction with a transparent input or output (`ssl-sign prepare --require-shielded`) before a FROST round starts.

## Results so far

| Check | Result |
| --- | --- |
| `ssl-sign` reproduces the sighash of a real v6 FROST-signed testnet tx (`a5533fe7…487d`) | ✅ |
| Our pinned stack parses PCZTs from the older (`pczt 0.8`) stack | ✅ |
| `apply` rejects a bad signature and an unsigned spend | ✅ |
| 2-of-3 RedPallas DKG through frostd; all three agree on the group key | ✅ |
| Rerandomized FROST signatures valid for all three pairs, coordinated by the non-signer | ✅ |
| Same signatures fail over a tampered sighash | ✅ |
| FROST-derived treasury address receives into the Ironwood pool | ✅ faucet drip arrived as an Ironwood note (block 4,524,498) |
| Treasury spent on testnet by each pair, zero transparent I/O | ✅ see below |

### On-chain spends (Zcash testnet, 2026-10-22)

| Signers | Coordinator | Transaction | Block | Spends signed |
| --- | --- | --- | --- | --- |
| op1 + op2 | op3 | [`09608b3d…254437`](https://testnet.zecblock.com/tx/09608b3d07f0515b2ef5c6783d8e13c3f1009504345d56184814daecca254437) | 4,529,462 | 1 |
| op2 + op3 | op1 | [`b421665c…13ee19`](https://testnet.zecblock.com/tx/b421665c27820b335fa434dafd2a1fd246bb668df4c8f2db89045d471e13ee19) | 4,529,470 | 1 |
| op1 + op3 | op2 | [`a0d1ef35…2b7e85`](https://testnet.zecblock.com/tx/a0d1ef351410f3350f96fb09be6f6b72c635d7cf35df917158767f16a52b7e85) | 4,529,480 | 2 (two notes, one FROST session) |

All three are v6 transactions with Ironwood spends and outputs only. `07-verify.sh` checks each one:
- It extracts the final transaction from the signed PCZT, which re-verifies every proof and signature.
- It confirms the final transaction has zero transparent inputs and outputs.
- It confirms the txid equals the one the network accepted. ZIP 244 txids commit to all effecting data, so a match means the checked transaction is the one on chain.
- It looks the txid up through lightwalletd to confirm it was mined.

Each payment went back to the treasury address so the test funds could be reused.

## Findings that affect the plan

- **Ironwood is NU6.3, not NU7.** It activated on mainnet on July 28, 2026, replacing Orchard after a circuit bug. `pczt ≥ 0.8` and `orchard ≥ 0.15` support it, including an external-signer path (`low_level_signer::sign_ironwood_with`, `Action::apply_signature`).
- **Quantum recoverability is unresolved for FROST keys.** Ironwood adds ZIP 2005 quantum-recoverable notes. Upstream FROST tooling derives the viewing key with a constructor that is explicitly *incompatible with quantum recoverability and will be removed*. ZF's guidance is to ignore this for now. It is a mainnet blocker to track, not a testnet one.
- **The coordinator needs no key share.** In every round the coordinating operator didn't sign. The production coordinator can run the coordinator role with public group data only, as the PRD requires.
- **frost-client stores key shares in plaintext config files**, by its own warning. Operator nodes must replace that with encrypted-at-rest storage (Phase 2 item 5).
- **Testnet funding:** the [Valar](https://faucet.testnet.valargroup.dev/) and [Jino Labs](https://zcashfaucet.jinolabs.xyz/) faucets work (0.1–0.125 TAZ per day).
- **Change needs about 3 confirmations before it can be spent again** (devtool's trusted-note policy). That is about 3–4 minutes on testnet, which caps the treasury at one spend per few minutes from a single note. The coordinator should keep several notes (devtool's `target_note_count`) so orders don't queue behind change.
- **`zcash-sign generate` is randomized.** Two runs for the same group key give different viewing keys and addresses. Treasury setup must generate once and persist the result; `03-treasury.sh` refuses to regenerate once a wallet exists.
- **Testnet is already on the NU7 consensus branch** (`inspect` reports branch `Nu7` for these transactions), so the spike exercises the current rules.
