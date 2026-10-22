import assert from "node:assert/strict";
import { test } from "node:test";
import { b58decode, b58encode, encodeFunctionCallTx } from "../src/sources/near-signer.js";

// Same layout as protocol/ssl-near's `function_call_encoding_matches_the_protocol_layout`.
test("function-call transaction encoding matches the protocol layout", () => {
  const bytes = encodeFunctionCallTx({
    signerId: "a",
    publicKey: new Uint8Array(32).fill(1),
    nonce: 5n,
    receiverId: "b",
    blockHash: new Uint8Array(32).fill(2),
    method: "m",
    args: new Uint8Array([9]),
    gas: 3n,
    deposit: 4n,
  });
  const u64 = (v: bigint) => {
    const b = Buffer.alloc(8);
    b.writeBigUInt64LE(v);
    return [...b];
  };
  const u128 = (v: bigint) => [...u64(v), ...u64(0n)];
  const expected = [
    1, 0, 0, 0, 0x61, 0,
    ...new Array(32).fill(1),
    ...u64(5n),
    1, 0, 0, 0, 0x62,
    ...new Array(32).fill(2),
    1, 0, 0, 0, 2, 1, 0, 0, 0, 0x6d, 1, 0, 0, 0, 9,
    ...u64(3n),
    ...u128(4n),
  ];
  assert.deepEqual([...bytes], expected);
});

test("base58 round-trips, including leading zeros", () => {
  for (const hex of ["00", "0001ff", "deadbeef", "00000000ffff"]) {
    const buf = Buffer.from(hex, "hex");
    assert.equal(b58decode(b58encode(buf)).toString("hex"), hex);
  }
  assert.equal(b58encode(Buffer.from("hello")), "Cn8eVZg");
});
