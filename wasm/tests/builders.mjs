import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);

const wasm = require("../pkg-node/chip35_dl_coin_wasm.js");
wasm.init();

const hexToBytes = (h) => Uint8Array.from(Buffer.from(h, "hex"));
const f = JSON.parse(readFileSync(new URL("./fixtures.json", import.meta.url)));

const synthKey = hexToBytes(f.syntheticKeyHex);
const ownerPh = hexToBytes(f.puzzleHashHex);
const adminInner = hexToBytes(f.puzzleHashHex); // admin DP from same key → same tree hash
const rootHash = hexToBytes(f.rootHashHex);

const coin = {
  parentCoinInfo: hexToBytes(f.parentCoinInfoHex),
  puzzleHash: ownerPh,
  amount: 2n,
};
const adminDp = { adminInnerPuzzleHash: adminInner };

// MINT
const mint = wasm.mintStore(
  synthKey, [coin], rootHash, "label", "desc", 42n, undefined, ownerPh, [adminDp], 0n
);
assert.ok(Array.isArray(mint.coinSpends) && mint.coinSpends.length > 0, "mint coinSpends");
assert.equal(Buffer.from(mint.newStore.metadata.rootHash).toString("hex"), f.rootHashHex, "mint rootHash");

// UPDATE METADATA (owner-authorized)
const upd = wasm.updateStoreMetadata(
  mint.newStore, hexToBytes("09".repeat(32)), "l2", undefined, undefined, undefined,
  synthKey, undefined, undefined
);
assert.ok(upd.coinSpends.length > 0, "update coinSpends");

// BURN (melt)
const melt = wasm.meltStore(mint.newStore, synthKey);
assert.equal(melt.length, 1, "melt one coin spend");

// SERIALIZATION round-trip (keyless; BLS G2 identity signature)
// BLS12-381 identity (infinity) point: 0xc0 followed by 95 zero bytes
const identitySig = new Uint8Array(96); identitySig[0] = 0xc0;
const hex = wasm.spendBundleToHex({ coinSpends: mint.coinSpends, aggregatedSignature: identitySig });
assert.equal(typeof hex, "string", "hex string");
const back = wasm.hexSpendBundleToCoinSpends(hex);
assert.equal(back.length, mint.coinSpends.length, "roundtrip coin spend count");

// DETERMINISM
const mint2 = wasm.mintStore(
  synthKey, [coin], rootHash, "label", "desc", 42n, undefined, ownerPh, [adminDp], 0n
);
const hex2 = wasm.spendBundleToHex({ coinSpends: mint2.coinSpends, aggregatedSignature: identitySig });
assert.equal(hex, hex2, "mint deterministic");

// NATIVE<->WASM GOLDEN PARITY
assert.equal(hex, f.mintHex, "wasm mint bundle hex == native golden (byte-for-byte parity)");

// UPDATE STORE OWNERSHIP
const own = wasm.updateStoreOwnership(mint.newStore, ownerPh, [adminDp], synthKey, undefined);
assert.ok(own.coinSpends.length > 0, "updateStoreOwnership");

// ORACLE SPEND — use a larger coin so amount >= oracleFee + fee + 1
const oracleCoin = {
  parentCoinInfo: hexToBytes(f.parentCoinInfoHex),
  puzzleHash: ownerPh,
  amount: 1000n,
};
const oracleDp = { oraclePaymentPuzzleHash: ownerPh, oracleFee: 2n };
const mintO = wasm.mintStore(
  synthKey, [oracleCoin], rootHash, "o", "o", undefined, undefined, ownerPh, [oracleDp], 0n
);
const oracle = wasm.oracleSpend(synthKey, [oracleCoin], mintO.newStore, 0n);
assert.ok(oracle.coinSpends.length > 0, "oracleSpend");

console.log("All chip35-dl-coin WASM builder checks passed.");
