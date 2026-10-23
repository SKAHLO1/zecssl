# Deploying ZEC SSL: website on Vercel, backend on Render

```
Browser ──► Vercel: web/ (Next.js)
               │  HTTPS + live stream
               ▼
            Render: zec-ssl-api (web service, public)
               │  private network
               ├──► zec-ssl-coordinator (private service, disk)
               ├──► zec-ssl-operator-1  (private service, disk, NEAR key 1)
               ├──► zec-ssl-operator-2  (private service, disk, NEAR key 2)
               └──► zec-ssl-operator-3  (private service, disk, NEAR key 3)
```

The coordinator and operators are private: only the API can reach them. Their state lives in SQLite files on Render persistent disks, mounted at `/var/data`. SQLite is the right store here:
- **The Zcash wallet library only supports SQLite.**
- **The coordinator must run as a single instance**, so SQLite's single writer costs nothing.
- **Render snapshots disks daily**, which gives you backups.

The API keeps no state of its own and needs no database.

## Costs and plans

Every backend service needs a **paid instance type**:
- Persistent disks require one.
- Free instances sleep when idle, which would stop deposit watching and the website's live stream.

The Blueprint uses `starter` for all five services, plus four 2 GB disks. Check Render's current pricing.

## Before you start

You need:
- **A deployed vault and token on NEAR testnet.** See [contracts/README.md](../contracts/README.md).
- **Each operator's NEAR account and secret key.** To print a key from your NEAR CLI keychain (it's a secret; paste it only into Render):
  ```bash
  python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["private_key"])' ~/.near-credentials/testnet/<operator>.testnet.json
  ```
- **The treasury's viewing key** (`TREASURY_UFVK`). For the Phase 2 treasury it's in `~/zec-ssl-spike/ufvk.txt`. It grants viewing, not spending, but keep it secret: it reveals every treasury flow.
- **The treasury wallet's birthday**: the block height it was created at (4524400 for the Phase 2 treasury).
- **Optionally, a faucet account**: any testnet account with a little NEAR, used to register users with test USDC and send them some.

## 1. Render (backend)

1. Push the repository to GitHub or GitLab.
2. In Render: **New → Blueprint**, pick the repository. Render reads [`render.yaml`](../render.yaml).
3. Render asks for the values marked `sync: false`:

   | Setting | Where | Value |
   | --- | --- | --- |
   | `NEAR_VAULT_CONTRACT` | group `zec-ssl-protocol`, and `zec-ssl-api` | e.g. `zecssl-skahlo-vault.testnet` |
   | `TREASURY_UFVK` | group | the treasury viewing key |
   | `WALLET_BIRTHDAY` | group | e.g. `4524400` |
   | `NEAR_ACCOUNT_ID`, `NEAR_SECRET_KEY` | each `zec-ssl-operator-N` | that operator's account and `ed25519:…` key |
   | `FROST_KEY_PACKAGE`, `FROST_PUBLIC_KEY_PACKAGE` | each `zec-ssl-operator-N` | that operator's FROST share, from `export_share` (below) |
   | `FROST_PUBLIC_KEY_PACKAGE` | `zec-ssl-coordinator` | the same public key package; lets the coordinator run refund rounds |
   | `OPERATOR_1_NAME` … `OPERATOR_3_NAME` | `zec-ssl-api` | each operator's NEAR account, in the same order |
   | `CORS_ORIGINS` | `zec-ssl-api` | your Vercel URL, e.g. `https://zec-ssl.vercel.app` (fill in after step 2) |
   | `NEAR_FAUCET_ACCOUNT`, `NEAR_FAUCET_KEY` | `zec-ssl-api` | optional faucet signer; leave empty to disable |
   | `COINGECKO_API_KEY` | `zec-ssl-api` | optional; raises CoinGecko's rate limit |

   Service addresses (`COORDINATOR_URL`, `OPERATOR_N_URL`) and the shared `OPERATOR_API_TOKEN` are filled in by Render.

   To print an operator's FROST values from its frost-client config (run where the share lives, and paste `FROST_KEY_PACKAGE` only into that operator's secret):

   ```bash
   cd protocol && cargo run -q -p ssl-frost --example export_share -- ~/zec-ssl-spike/operators/op1.toml "$(cat ~/zec-ssl-spike/group.key)"
   ```
4. Apply. The first Rust image build takes a while, because it compiles the Zcash and FROST libraries. Later builds reuse Render's build cache.
5. Check `https://<zec-ssl-api>.onrender.com/v1/health`. All sources should report `ok`. The coordinator and operators need a minute after their first start to import the viewing key and sync.

**Before Render goes live, stop any local copies** of the operators and coordinator (`protocol/run/start-local.sh`). Two processes signing with the same NEAR key race for transaction nonces.

Orders that the local operators approved and that are still open won't be released by the Render operators, which start with fresh state. Let them settle or expire first.

Order IDs and deposit addresses include randomness, so a fresh coordinator can't collide with orders already on the vault. As a second guard, operators refuse to approve an ID the vault already holds with different terms.

## 2. Vercel (website)

1. **New Project** → import the same repository.
2. **Root Directory:** `web`. Leave "Include files outside the root directory" on: the website imports the API's type definitions from `server/`.
3. Framework preset: Next.js. Install and build commands: defaults; pnpm is detected from the lockfile.
4. Environment variables, for Production and Preview:
   ```
   NEXT_PUBLIC_API_URL=https://<zec-ssl-api>.onrender.com
   API_URL=https://<zec-ssl-api>.onrender.com
   ```
5. Deploy, then put the Vercel URL into `CORS_ORIGINS` on `zec-ssl-api` and redeploy the API.

## NEAR RPC limits

Everything defaults to FastNEAR's public testnet RPC, which rate-limits (HTTP 429) under sustained use. The services retry with backoff and keep their polling light, but a public deployment should use a dedicated endpoint:
- a FastNEAR subscription, or another NEAR RPC provider;
- set `NEAR_RPC_URL` in the `zec-ssl-protocol` group and on `zec-ssl-api`.

## Operating notes

- **Never run more than one instance** of the coordinator or of any operator; each has its own disk.
- **Disks are the operators' memory.** An operator that loses its disk forgets which orders it approved. Its wallet rebuilds from the chain, but it won't release orders it approved before the loss. Restore from Render's snapshot.
- **This is not yet independent operation.** All three operators in one Render account is fine for testnet; independent operators on separate infrastructure is plan Phase 8.
- **Each operator's FROST share is a secret of that operator alone.** Without shares, orders still settle, but underpaid or late deposits can't be refunded. On testnet the three shares sit in one Render account; Phase 8 moves them to separate operators.
