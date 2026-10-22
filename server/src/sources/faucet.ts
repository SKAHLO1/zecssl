import { config } from "../config.js";
import { HttpError } from "../http.js";
import { store } from "../store.js";
import { fetchAccount, viewCall } from "./near.js";
import { callFunction, loadFaucetSigner, type NearSigner } from "./near-signer.js";

/*
 * Testnet-only helpers for the vault's test token (contracts/test-token):
 * - register a NEAR account with the token so swap payouts can reach it;
 * - send it test USDC.
 * The faucet account pays the token's storage deposit and holds a test-USDC float it
 * mints for itself. Never enabled off testnet.
 */

const STORAGE_DEPOSIT_YOCTO = 1_250_000_000_000_000_000_000n; // 0.00125 NEAR
const DRIP_TOKENS = 100;
const ONE_DAY = 24 * 3600 * 1000;

let signer: NearSigner | null | undefined;

function faucetSigner(): NearSigner {
  if (config.nearNetwork !== "testnet") throw new HttpError(404, "the faucet only exists on testnet");
  if (signer === undefined) signer = loadFaucetSigner();
  if (!signer) throw new HttpError(503, "the faucet is not configured (set NEAR_FAUCET_ACCOUNT and NEAR_FAUCET_KEY)");
  return signer;
}

function vaultToken(): { contract: string; decimals: number; symbol: string } {
  const t = store.state.network?.vaultToken;
  if (!t?.contractAddress) throw new HttpError(503, "vault token unknown; is NEAR_VAULT_CONTRACT set?");
  return { contract: t.contractAddress, decimals: t.decimals, symbol: t.symbol };
}

export function faucetEnabled(): boolean {
  try {
    faucetSigner();
    return true;
  } catch {
    return false;
  }
}

const NEAR_ACCOUNT = /^(?=.{2,64}$)(([a-z\d]+[-_])*[a-z\d]+\.)*([a-z\d]+[-_])*[a-z\d]+$/;

export interface AccountStatus {
  accountId: string;
  exists: boolean;
  /** Registered with the vault token, so payouts reach it directly. */
  registered: boolean;
  balance: string | null;
  symbol: string;
}

export async function accountStatus(accountId: string): Promise<AccountStatus> {
  if (!NEAR_ACCOUNT.test(accountId) && !/^[0-9a-f]{64}$/.test(accountId)) throw new HttpError(400, "not a valid NEAR account id");
  const token = vaultToken();
  const { account } = await fetchAccount(accountId);
  const [storage, balance] = account.exists
    ? await Promise.all([
        viewCall<unknown>(token.contract, "storage_balance_of", { account_id: accountId }),
        viewCall<string>(token.contract, "ft_balance_of", { account_id: accountId }),
      ])
    : [null, null];
  return {
    accountId,
    exists: account.exists,
    registered: storage != null,
    balance: balance == null ? null : String(Number(balance) / 10 ** token.decimals),
    symbol: token.symbol,
  };
}

// Simple in-memory limits: one drip per account and five per client per day.
const byAccount = new Map<string, number>();
const byClient = new Map<string, number[]>();

function rateLimit(accountId: string, client: string) {
  const now = Date.now();
  const last = byAccount.get(accountId);
  if (last && now - last < ONE_DAY) throw new HttpError(429, "this account already received test USDC in the last 24 hours");
  const recent = (byClient.get(client) ?? []).filter((t) => now - t < ONE_DAY);
  if (recent.length >= 5) throw new HttpError(429, "too many faucet requests from this client today");
  byAccount.set(accountId, now);
  byClient.set(client, [...recent, now]);
}

export async function register(accountId: string): Promise<{ txHash: string | null; status: AccountStatus }> {
  const s = faucetSigner();
  const status = await accountStatus(accountId);
  if (!status.exists) throw new HttpError(404, "that NEAR testnet account does not exist");
  if (status.registered) return { txHash: null, status };
  const { contract } = vaultToken();
  const { txHash } = await callFunction(s, contract, "storage_deposit", { account_id: accountId, registration_only: true }, 30, STORAGE_DEPOSIT_YOCTO);
  return { txHash, status: await accountStatus(accountId) };
}

export async function drip(accountId: string, client: string): Promise<{ txHash: string; amount: number; status: AccountStatus }> {
  const s = faucetSigner();
  const token = vaultToken();
  const status = await accountStatus(accountId);
  if (!status.exists) throw new HttpError(404, "that NEAR testnet account does not exist");
  rateLimit(accountId, client);
  if (!status.registered) await callFunction(s, token.contract, "storage_deposit", { account_id: accountId, registration_only: true }, 30, STORAGE_DEPOSIT_YOCTO);

  const units = BigInt(DRIP_TOKENS) * 10n ** BigInt(token.decimals);
  const float = BigInt((await viewCall<string>(token.contract, "ft_balance_of", { account_id: s.accountId })) ?? "0");
  if (float < units) {
    // Top up the faucet's own float from the token's public faucet (10,000 per call). The
    // token only takes a storage deposit the first time, when the faucet isn't registered.
    const registered = (await viewCall<unknown>(token.contract, "storage_balance_of", { account_id: s.accountId })) != null;
    await callFunction(s, token.contract, "faucet", { amount: String(10_000n * 10n ** BigInt(token.decimals)) }, 30, registered ? 0n : STORAGE_DEPOSIT_YOCTO);
  }
  const { txHash } = await callFunction(s, token.contract, "ft_transfer", { receiver_id: accountId, amount: String(units), memo: "zec-ssl testnet faucet" }, 30, 1n);
  return { txHash, amount: DRIP_TOKENS, status: await accountStatus(accountId) };
}
