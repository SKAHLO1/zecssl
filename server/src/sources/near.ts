import { config } from "../config.js";
import { fetchJson, HttpError } from "../http.js";
import type { NearChain, VaultAccount } from "../types.js";

interface RpcResponse<T> {
  result?: T;
  error?: { name?: string; cause?: { name?: string }; message?: string; data?: string };
}

async function rpc<T>(method: string, params: unknown) {
  const { data, latencyMs } = await fetchJson<RpcResponse<T>>(config.nearRpcUrl, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: "zec-ssl", method, params }),
  });
  if (data.error) {
    const cause = data.error.cause?.name ?? data.error.name ?? "RPC_ERROR";
    throw new HttpError(400, `${cause}: ${data.error.data ?? data.error.message ?? ""}`.trim());
  }
  return { result: data.result as T, latencyMs };
}

interface NearBlock {
  header: { height: number; hash: string; timestamp_nanosec: string; gas_price: string; latest_protocol_version?: number };
}

interface NearStatus {
  chain_id: string;
  protocol_version: number;
}

let chainId: string | null = null;
let protocolVersion: number | null = null;

export async function fetchNearChain(): Promise<{ chain: NearChain; latencyMs: number }> {
  if (!chainId) {
    const { result } = await rpc<NearStatus>("status", []);
    chainId = result.chain_id;
    protocolVersion = result.protocol_version;
  }
  const { result, latencyMs } = await rpc<NearBlock>("block", { finality: "final" });
  const h = result.header;
  return {
    latencyMs,
    chain: {
      network: config.nearNetwork,
      chainId: chainId ?? config.nearNetwork,
      height: h.height,
      blockHash: h.hash,
      blockTime: new Date(Number(BigInt(h.timestamp_nanosec) / 1_000_000n)).toISOString(),
      gasPrice: h.gas_price,
      protocolVersion,
      updatedAt: new Date().toISOString(),
    },
  };
}

interface ViewAccount {
  amount: string;
  storage_usage: number;
  code_hash: string;
}

const YOCTO = 10n ** 24n;

function formatNear(yocto: string): string {
  const v = BigInt(yocto);
  const whole = v / YOCTO;
  const frac = ((v % YOCTO) * 10_000n) / YOCTO;
  return `${whole}.${frac.toString().padStart(4, "0")}`;
}

/** Reads a deployed contract's account state. A missing account is reported, not thrown. */
export async function fetchAccount(contract: string): Promise<{ account: VaultAccount; latencyMs: number }> {
  try {
    const { result, latencyMs } = await rpc<ViewAccount>("query", {
      request_type: "view_account",
      finality: "final",
      account_id: contract,
    });
    return {
      latencyMs,
      account: {
        contract,
        network: config.nearNetwork,
        exists: true,
        balanceNear: formatNear(result.amount),
        storageBytes: result.storage_usage,
        codeHash: result.code_hash,
      },
    };
  } catch (err) {
    if (err instanceof HttpError && /UNKNOWN_ACCOUNT/.test(err.message)) {
      return {
        latencyMs: 0,
        account: { contract, network: config.nearNetwork, exists: false, balanceNear: null, storageBytes: null, codeHash: null },
      };
    }
    throw err;
  }
}

/** Calls a view method on a contract; returns null when the method does not exist. */
export async function viewCall<T>(contract: string, method: string, args: object = {}): Promise<T | null> {
  try {
    const { result } = await rpc<{ result: number[] }>("query", {
      request_type: "call_function",
      finality: "final",
      account_id: contract,
      method_name: method,
      args_base64: Buffer.from(JSON.stringify(args)).toString("base64"),
    });
    return JSON.parse(Buffer.from(result.result).toString("utf8")) as T;
  } catch (err) {
    if (err instanceof HttpError && /MethodNotFound|CodeDoesNotExist|MethodResolveError/.test(err.message)) return null;
    throw err;
  }
}
