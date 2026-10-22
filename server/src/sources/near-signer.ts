import { createHash, createPrivateKey, sign as edSign, type KeyObject } from "node:crypto";
import { readFileSync } from "node:fs";
import { basename, dirname } from "node:path";
import { config } from "../config.js";
import { fetchJson, HttpError } from "../http.js";

/*
 * Signs NEAR function calls for the testnet faucet. Same wire format as the Rust services
 * (protocol/ssl-near): a borsh-encoded v0 transaction, signed as ed25519 over its SHA-256.
 * Node's own crypto does the signing; no key material leaves this process.
 */

const ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

export function b58decode(s: string): Buffer {
  let n = 0n;
  for (const c of s) {
    const v = ALPHABET.indexOf(c);
    if (v < 0) throw new Error("invalid base58");
    n = n * 58n + BigInt(v);
  }
  const bytes: number[] = [];
  while (n > 0n) {
    bytes.unshift(Number(n & 0xffn));
    n >>= 8n;
  }
  for (const c of s) {
    if (c !== "1") break;
    bytes.unshift(0);
  }
  return Buffer.from(bytes);
}

export function b58encode(buf: Uint8Array): string {
  let n = BigInt(`0x${Buffer.from(buf).toString("hex") || "0"}`);
  let out = "";
  while (n > 0n) {
    out = ALPHABET[Number(n % 58n)] + out;
    n /= 58n;
  }
  for (const b of buf) {
    if (b !== 0) break;
    out = `1${out}`;
  }
  return out;
}

// ---------- borsh ----------

class Writer {
  private parts: Buffer[] = [];
  u8(v: number) {
    this.parts.push(Buffer.from([v]));
    return this;
  }
  u32(v: number) {
    const b = Buffer.alloc(4);
    b.writeUInt32LE(v);
    this.parts.push(b);
    return this;
  }
  u64(v: bigint) {
    const b = Buffer.alloc(8);
    b.writeBigUInt64LE(v);
    this.parts.push(b);
    return this;
  }
  u128(v: bigint) {
    const b = Buffer.alloc(16);
    b.writeBigUInt64LE(v & 0xffffffffffffffffn);
    b.writeBigUInt64LE(v >> 64n, 8);
    this.parts.push(b);
    return this;
  }
  bytes(v: Uint8Array) {
    this.parts.push(Buffer.from(v));
    return this;
  }
  vec(v: Uint8Array) {
    return this.u32(v.length).bytes(v);
  }
  string(s: string) {
    return this.vec(Buffer.from(s, "utf8"));
  }
  done() {
    return Buffer.concat(this.parts);
  }
}

/** Borsh bytes of a v0 transaction with one FunctionCall action (enum index 2). */
export function encodeFunctionCallTx(tx: {
  signerId: string;
  publicKey: Uint8Array;
  nonce: bigint;
  receiverId: string;
  blockHash: Uint8Array;
  method: string;
  args: Uint8Array;
  gas: bigint;
  deposit: bigint;
}): Buffer {
  return new Writer()
    .string(tx.signerId)
    .u8(0)
    .bytes(tx.publicKey)
    .u64(tx.nonce)
    .string(tx.receiverId)
    .bytes(tx.blockHash)
    .u32(1)
    .u8(2)
    .string(tx.method)
    .vec(tx.args)
    .u64(tx.gas)
    .u128(tx.deposit)
    .done();
}

// ---------- signer ----------

export interface NearSigner {
  accountId: string;
  publicKey: Buffer;
  key: KeyObject;
}

const PKCS8_ED25519_PREFIX = Buffer.from("302e020100300506032b657004220420", "hex");

export function signerFromSecret(accountId: string, secret: string): NearSigner {
  const raw = b58decode(secret.replace(/^ed25519:/, ""));
  if (raw.length !== 64) throw new Error("expected a 64-byte ed25519 secret key");
  const key = createPrivateKey({ key: Buffer.concat([PKCS8_ED25519_PREFIX, raw.subarray(0, 32)]), format: "der", type: "pkcs8" });
  return { accountId, publicKey: raw.subarray(32), key };
}

/**
 * The faucet's signer: NEAR_FAUCET_ACCOUNT + NEAR_FAUCET_KEY (e.g. Render secrets), or
 * NEAR_FAUCET_CREDENTIALS pointing at a NEAR CLI keychain file. `null` when not configured.
 */
export function loadFaucetSigner(): NearSigner | null {
  if (config.faucetAccount && config.faucetKey) return signerFromSecret(config.faucetAccount, config.faucetKey);
  if (!config.faucetCredentials) return null;
  const file = JSON.parse(readFileSync(config.faucetCredentials, "utf8")) as { account_id?: string; private_key?: string };
  const stem = basename(config.faucetCredentials, ".json");
  const account = file.account_id ?? (stem.startsWith("ed25519_") ? basename(dirname(config.faucetCredentials)) : stem);
  if (!file.private_key) throw new Error("credentials file has no private_key");
  return signerFromSecret(account, file.private_key);
}

interface RpcResponse<T> {
  result?: T;
  error?: { data?: unknown; message?: string };
}

async function rpc<T>(method: string, params: unknown): Promise<T> {
  const { data } = await fetchJson<RpcResponse<T>>(config.nearRpcUrl, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: "faucet", method, params }),
    timeoutMs: 60_000,
  });
  if (data.error) throw new HttpError(502, `${method}: ${JSON.stringify(data.error.data ?? data.error.message)}`);
  return data.result as T;
}

let queue: Promise<unknown> = Promise.resolve();

/** Signs and sends one function call and waits until final. Calls are serialized per process. */
export function callFunction(signer: NearSigner, receiverId: string, method: string, args: object, gasTgas: number, depositYocto: bigint) {
  const run = async () => {
    const pk = `ed25519:${b58encode(signer.publicKey)}`;
    const ak = await rpc<{ nonce: number }>("query", { request_type: "view_access_key", finality: "final", account_id: signer.accountId, public_key: pk });
    const block = await rpc<{ header: { hash: string } }>("block", { finality: "final" });
    const bytes = encodeFunctionCallTx({
      signerId: signer.accountId,
      publicKey: signer.publicKey,
      nonce: BigInt(ak.nonce) + 1n,
      receiverId,
      blockHash: b58decode(block.header.hash),
      method,
      args: Buffer.from(JSON.stringify(args)),
      gas: BigInt(gasTgas) * 1_000_000_000_000n,
      deposit: depositYocto,
    });
    const digest = createHash("sha256").update(bytes).digest();
    const signature = edSign(null, digest, signer.key);
    const signed = Buffer.concat([bytes, Buffer.from([0]), signature]);
    const result = await rpc<{ status: { Failure?: unknown; SuccessValue?: string } }>("send_tx", {
      signed_tx_base64: signed.toString("base64"),
      wait_until: "FINAL",
    });
    if (result.status.Failure) throw new HttpError(502, `${method} failed: ${JSON.stringify(result.status.Failure)}`);
    return { txHash: b58encode(digest) };
  };
  const p = queue.then(run, run);
  queue = p.catch(() => undefined);
  return p;
}
