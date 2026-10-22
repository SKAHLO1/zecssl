# ZEC SSL protocol

Rust code that touches keys and chains. This currently holds the Phase 2 signing spike from [docs/testnet-plan.md](../docs/testnet-plan.md): proving that a 2-of-3 rerandomized FROST key can spend from a shielded treasury on Zcash testnet.

```
protocol/
├── ssl-sign/          prepare a PCZT for external signing, apply and verify signatures, extract the final tx
├── spike/             end-to-end scripts: frostd, DKG, treasury wallet, 2-of-3 spends
└── scripts/           build the pinned upstream tools; build and test ssl-sign
```

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
