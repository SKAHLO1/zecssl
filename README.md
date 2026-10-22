# ZEC SSL — Shielded Settlement Layer

Web app and API for ZEC SSL, a bonded, threshold-signed operator network that settles cross-chain swaps directly into and out of Zcash's shielded pool.

```
zec-ssl/
├── server/   long-running API (Fastify): pollers, cache, REST and a live SSE stream
└── web/      Next.js app: homepage and dashboard (swap, operators, treasury, markets, activity, status)
```

The web app never calls third-party APIs itself. The server polls every source on a schedule, keeps the latest snapshot in memory, and pushes changes to browsers over Server-Sent Events, so page views never touch upstream rate limits.

## Deploy

Website on Vercel, backend on Render: see [docs/deploy.md](docs/deploy.md) and [render.yaml](render.yaml).

## Run it

Requires Node 22+ and pnpm 10.

```bash
pnpm install
cp server/.env.example server/.env
cp web/.env.example web/.env.local
pnpm dev:server   # API on http://localhost:4000
pnpm dev:web      # app on http://localhost:3000
```

Production: `pnpm build`, then `node --env-file=.env dist/index.js` in `server/` and `next start` in `web/`.

## Data sources

All data is live. Nothing in the app is mocked: anything not yet deployed shows an explicit "not connected" state.

| Data | Source | Poll |
| --- | --- | --- |
| Prices, 7-day sparklines, charts | CoinGecko (`COINGECKO_API_KEY` optional) | 60 s; charts cached per range |
| Zcash chain head, blocks, tx sample | Your Zebra node (`ZEBRA_RPC_URL`), else Blockchair, else 3xpl | 30 s |
| NEAR chain head | NEAR RPC (`NEAR_RPC_URL`, testnet by default) | 6 s |
| Routable assets and quote pricing | NEAR Intents 1Click `/v0/tokens` (and `/v0/quote` dry runs) | 60 s |
| Orders, signing rounds, events, treasury | ZEC SSL coordinator (`COORDINATOR_URL`) | 10 s |
| Operator health | Each operator's `/health` (`OPERATOR_ENDPOINTS`) | 10 s |
| Vault and bond contracts | NEAR RPC view calls (`NEAR_VAULT_CONTRACT`, `NEAR_BONDS_CONTRACT`) | 10 s |

Blockchair's keyless tier is small; the server polls it slowly and pauses for 30 minutes if it rate-limits, falling back to 3xpl for the chain head. Point `ZEBRA_RPC_URL` at the operators' Zebra node for full data, including shielded value pools.

Quotes: with `INTENTS_REFERENCE_REFUND` set, the NEAR leg is priced by a dry-run 1Click router quote; otherwise from 1Click's live oracle prices. The operator spread (`QUOTE_SPREAD_BPS`) and the ZIP-317 network fee are applied either way.

## API

| Method | Path | Returns |
| --- | --- | --- |
| GET | `/v1/health` | uptime and per-source status |
| GET | `/v1/overview` | every snapshot in one response (used for the first render) |
| GET | `/v1/stream` | SSE: `market`, `zcash`, `near`, `intents`, `network`, `sources` events |
| GET | `/v1/market` | tracked assets |
| GET | `/v1/market/:id/chart?range=1D\|7D\|30D\|1Y` | price history |
| GET | `/v1/zcash`, `/v1/near`, `/v1/intents/tokens`, `/v1/network` | individual snapshots |
| POST | `/v1/quote` | `{ amount, destinationAssetId, recipient? }` → quote |
| POST | `/v1/orders` | forwarded to the coordinator; 503 until one is configured |

Response types are in [server/src/types.ts](server/src/types.ts).

## Contract for ZEC SSL services

Implement these and set the matching env vars; the dashboard lights up without code changes.

**Coordinator** (`COORDINATOR_URL`)

- `GET /v1/orders?limit=50` → `Order[]`
- `GET /v1/signing-rounds?limit=50` → `SigningRound[]`
- `GET /v1/events?limit=50` → `NetworkEvent[]`
- `GET /v1/treasury` → `{ shieldedBalanceZat, addressesIssued, lastScanHeight }`
- `POST /v1/orders` → `{ id, depositAddress, memo?, expiresAt? }`

**Operator node** (`OPERATOR_ENDPOINTS=op-1=https://…,op-2=https://…`)

- `GET /health` → `{ status: "ok" | "degraded", version?, nearAccount?, bondUsd?, signaturesTotal?, lastSignatureAt? }`

**NEAR contracts**

- Vault: view method `get_liquidity` → `[{ symbol, amount }]`
- Bonds: view method `get_total_bonded` → `{ usd }`
