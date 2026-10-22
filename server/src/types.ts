// Response shapes served by the API. The web app imports these as types only.

export type SourceName = "coingecko" | "zcash" | "near" | "intents" | "coordinator" | "operators" | "vault";

export interface SourceStatus {
  name: SourceName;
  label: string;
  /** false when the source is not configured (e.g. no coordinator URL yet) */
  configured: boolean;
  ok: boolean;
  lastSuccess: string | null;
  lastError: string | null;
  latencyMs: number | null;
}

export interface MarketAsset {
  id: string;
  symbol: string;
  name: string;
  image: string;
  rank: number | null;
  price: number;
  change1h: number | null;
  change24h: number | null;
  change7d: number | null;
  marketCap: number;
  volume24h: number;
  high24h: number | null;
  low24h: number | null;
  circulatingSupply: number | null;
  ath: number | null;
  sparkline7d: number[];
  updatedAt: string;
}

export type ChartRange = "1D" | "7D" | "30D" | "1Y";

export interface PricePoint {
  t: number;
  price: number;
}

export interface PriceChart {
  id: string;
  range: ChartRange;
  points: PricePoint[];
  updatedAt: string;
}

export type ZcashTxKind = "coinbase" | "shielded" | "shielding" | "deshielding" | "transparent";

export interface ZcashBlock {
  height: number;
  hash: string;
  time: string;
  txCount: number;
  size: number;
}

export interface ZcashTx {
  hash: string;
  height: number;
  time: string;
  version: number;
  kind: ZcashTxKind;
  inputCount: number;
  outputCount: number;
  /** transparent output value in zatoshis */
  transparentOutZat: number;
  feeZat: number;
}

export interface ZcashValuePool {
  id: string;
  valueZec: number;
}

export interface ZcashChain {
  /** which provider produced this snapshot */
  source: "zebra" | "blockchair" | "3xpl";
  height: number;
  bestBlockHash: string;
  bestBlockTime: string;
  blocks24h: number | null;
  txs24h: number | null;
  avgBlockTimeSec: number | null;
  mempoolTxs: number | null;
  hashrate: number | null;
  nodes: number | null;
  avgFeeUsd: number | null;
  blockchainSizeBytes: number | null;
  /** chain value per pool; only a full node reports these */
  valuePools: ZcashValuePool[] | null;
  recentBlocks: ZcashBlock[];
  recentTxs: ZcashTx[];
  /** share of the recent sample by kind, coinbase excluded; null when the source cannot classify */
  txMix: Record<Exclude<ZcashTxKind, "coinbase">, number> | null;
  updatedAt: string;
}

export interface NearChain {
  network: string;
  chainId: string;
  height: number;
  blockHash: string;
  blockTime: string;
  gasPrice: string;
  protocolVersion: number | null;
  updatedAt: string;
}

export interface IntentsToken {
  assetId: string;
  symbol: string;
  blockchain: string;
  decimals: number;
  price: number;
  contractAddress: string | null;
}

export interface IntentsSnapshot {
  tokens: IntentsToken[];
  chains: string[];
  zec: IntentsToken | null;
  updatedAt: string;
}

export interface QuoteRequest {
  /** ZEC amount as a decimal string */
  amount: string;
  destinationAssetId: string;
  recipient?: string;
}

export interface Quote {
  source: "router" | "oracle";
  amountIn: string;
  amountInUsd: number;
  amountOut: string;
  amountOutUsd: number;
  minAmountOut: string | null;
  destination: IntentsToken;
  rate: number;
  spreadBps: number;
  operatorFee: string;
  zcashNetworkFeeZec: string;
  timeEstimateSec: number | null;
  quotedAt: string;
}

export type OperatorState = "online" | "degraded" | "offline";

export interface Operator {
  id: string;
  name: string;
  endpoint: string;
  state: OperatorState;
  latencyMs: number | null;
  version: string | null;
  /** fields below come from the operator's /health response when it reports them */
  nearAccount: string | null;
  bondUsd: number | null;
  signaturesTotal: number | null;
  lastSignatureAt: string | null;
  lastSeen: string | null;
}

export type OrderState =
  | "quoted"
  | "awaiting_deposit"
  | "deposit_detected"
  | "confirming"
  | "signing"
  | "released"
  | "refunded"
  | "expired"
  | "disputed";

export interface Order {
  id: string;
  direction: "zec_to_near" | "near_to_zec";
  state: OrderState;
  amountInZec: string;
  amountOut: string;
  destinationSymbol: string;
  signatures: number;
  threshold: number;
  createdAt: string;
  updatedAt: string;
  nearTxHash: string | null;
}

export interface SigningRound {
  id: string;
  orderId: string;
  kind: "dkg" | "zcash_spend" | "near_release" | "refund";
  signers: string[];
  collected: number;
  threshold: number;
  state: "open" | "complete" | "failed";
  startedAt: string;
  completedAt: string | null;
}

export interface NetworkEvent {
  id: string;
  kind: "order" | "signing" | "deposit" | "release" | "refund" | "slash" | "operator";
  title: string;
  detail: string;
  at: string;
}

export interface TreasuryState {
  shieldedBalanceZat: string | null;
  addressesIssued: number | null;
  lastScanHeight: number | null;
  vaultLiquidity: { symbol: string; amount: string }[];
  totalBondedUsd: number | null;
}

export interface VaultAccount {
  contract: string;
  network: string;
  exists: boolean;
  balanceNear: string | null;
  storageBytes: number | null;
  codeHash: string | null;
}

export interface NetworkSnapshot {
  threshold: { t: number; n: number };
  coordinator: { configured: boolean; url: string | null; online: boolean; latencyMs: number | null };
  operators: Operator[];
  vault: VaultAccount | null;
  bonds: VaultAccount | null;
  treasury: TreasuryState | null;
  orders: Order[];
  rounds: SigningRound[];
  events: NetworkEvent[];
  updatedAt: string;
}

export interface Overview {
  market: MarketAsset[];
  zcash: ZcashChain | null;
  near: NearChain | null;
  intents: IntentsSnapshot | null;
  network: NetworkSnapshot;
  sources: SourceStatus[];
}

export type StreamTopic = "market" | "zcash" | "near" | "intents" | "network" | "sources";
