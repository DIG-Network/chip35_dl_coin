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

// ===========================================================================
// Delegation builders (hub Teams #43 + revocable deploy tokens #17)
// ===========================================================================

// adminDelegatedPuzzleFromKey / writerDelegatedPuzzleFromKey / oracleDelegatedPuzzle
const adminFromKey = wasm.adminDelegatedPuzzleFromKey(synthKey);
assert.ok(adminFromKey.adminInnerPuzzleHash, "admin DP has adminInnerPuzzleHash");
// admin DP from the synthetic key currying = the synthetic key's standard puzzle hash = ownerPh
assert.equal(
  Buffer.from(adminFromKey.adminInnerPuzzleHash).toString("hex"),
  f.puzzleHashHex,
  "admin DP tree hash == StandardArgs::curry_tree_hash(syntheticKey)"
);

const writerFromKey = wasm.writerDelegatedPuzzleFromKey(synthKey);
assert.ok(writerFromKey.writerInnerPuzzleHash, "writer DP has writerInnerPuzzleHash");
assert.equal(
  Buffer.from(writerFromKey.writerInnerPuzzleHash).toString("hex"),
  f.puzzleHashHex,
  "writer DP shares the standard-puzzle tree hash (keyed only by the synthetic key)"
);

const oracleFromBuilder = wasm.oracleDelegatedPuzzle(ownerPh, 7n);
assert.equal(
  Buffer.from(oracleFromBuilder.oraclePaymentPuzzleHash).toString("hex"),
  f.puzzleHashHex,
  "oracle DP payment puzzle hash"
);
assert.equal(oracleFromBuilder.oracleFee, 7n, "oracle DP fee");

// TEAMS (#43) / DEPLOY TOKEN (#17): mint owner-only, then ISSUE a writer delegate (deploy token)
// via updateStoreOwnership, then the writer ADVANCES the root (deploy) with no owner seed.
const teamMint = wasm.mintStore(
  synthKey, [coin], rootHash, "team", "team", undefined, undefined, ownerPh, [], 0n
);
assert.equal(teamMint.newStore.delegatedPuzzles.length, 0, "team store starts owner-only");

const issued = wasm.updateStoreOwnership(
  teamMint.newStore, ownerPh, [writerFromKey], synthKey, undefined
);
assert.equal(issued.newStore.delegatedPuzzles.length, 1, "deploy token issued (writer delegate)");
assert.ok(
  issued.newStore.delegatedPuzzles[0].writerInnerPuzzleHash,
  "issued delegate is a writer"
);

// The writer (deploy key) advances the root WITHOUT the owner seed → writerPublicKey arg.
const deployed = wasm.updateStoreMetadata(
  issued.newStore, hexToBytes("09".repeat(32)), undefined, undefined, undefined, undefined,
  undefined, undefined, synthKey // writerPublicKey
);
assert.ok(deployed.coinSpends.length > 0, "writer advances root (deploy) without owner seed");
assert.equal(
  Buffer.from(deployed.newStore.metadata.rootHash).toString("hex"),
  "09".repeat(32),
  "deploy advanced the store to the new capsule root"
);

// REVOKE: owner replaces the delegated set, dropping the writer.
const revoked = wasm.updateStoreOwnership(
  deployed.newStore, ownerPh, [], synthKey, undefined
);
assert.equal(revoked.newStore.delegatedPuzzles.length, 0, "deploy token revoked");

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

// ADD FEE
const fee = wasm.addFee(synthKey, [coin], [new Uint8Array(32).fill(5)], 1n);
assert.ok(Array.isArray(fee) && fee.length > 0, "addFee returns coin spends");

// ===========================================================================
// Asset toolkit exports (roadmap #33/#34/#35/#36)
// ===========================================================================

// --- #36: sha256 + CHIP-0007 metadata builder + validator ---
const dataBytes = Buffer.from("the real PNG bytes in a DIG capsule");
const dataHash = wasm.sha256(dataBytes);
assert.equal(dataHash.length, 32, "sha256 returns 32 bytes");

const built = wasm.buildChip0007Metadata({
  name: "DIG Punk #1",
  description: "first",
  attributes: [{ traitType: "Background", value: "Blue" }],
});
assert.equal(typeof built.json, "string", "metadata json string");
assert.ok(JSON.parse(built.json).format === "CHIP-0007", "format defaulted to CHIP-0007");
assert.equal(built.metadataHash.length, 32, "metadataHash 32 bytes");
// metadataHash == sha256(json) (reproducible)
assert.equal(
  Buffer.from(built.metadataHash).toString("hex"),
  Buffer.from(wasm.sha256(Buffer.from(built.json))).toString("hex"),
  "metadataHash == sha256(canonical json)"
);

// validate: matching bytes pass, mismatched fail
const okV = wasm.validateChip0007({ name: "x" }, { dataBytes, dataHash });
assert.equal(okV.ok, true, "validate passes for matching data hash");
const badV = wasm.validateChip0007({ name: "x" }, { dataBytes, dataHash: new Uint8Array(32) });
assert.equal(badV.ok, false, "validate fails for mismatched data hash");
assert.ok(badV.errors.length > 0, "validate reports an error");

// --- #33: mint an NFT with dig:// + https fallback URIs and computed hashes ---
const nftParams = {
  metadata: {
    dataUris: [
      "dig://urn:dig:chia:store:root/art.png",
      "https://gateway.dig.net/store/root/art.png",
    ],
    dataHash,
    metadataUris: ["dig://urn:dig:chia:store:root/metadata.json"],
    metadataHash: built.metadataHash,
    licenseUris: [],
    editionNumber: 1n,
    editionTotal: 1n,
  },
  p2PuzzleHash: ownerPh,
  royaltyPuzzleHash: ownerPh,
  royaltyBasisPoints: 300,
};
const nft = wasm.mintNft(synthKey, [coin], nftParams, 0n);
assert.ok(nft.coinSpends.length > 0, "mintNft coinSpends");
assert.equal(nft.launcherId.length, 32, "mintNft launcherId 32 bytes");

// determinism
const nft2 = wasm.mintNft(synthKey, [coin], nftParams, 0n);
const nh1 = wasm.spendBundleToHex({ coinSpends: nft.coinSpends, aggregatedSignature: identitySig });
const nh2 = wasm.spendBundleToHex({ coinSpends: nft2.coinSpends, aggregatedSignature: identitySig });
assert.equal(nh1, nh2, "mintNft deterministic");

// --- #35: createDid ---
const did = wasm.createDid(synthKey, [coin], 0n);
assert.ok(did.coinSpends.length > 0, "createDid coinSpends");
assert.equal(did.launcherId.length, 32, "did launcherId 32 bytes");
assert.equal(did.innerPuzzleHash.length, 32, "did innerPuzzleHash 32 bytes");

// --- #35: issueCat ---
const catCoin = { parentCoinInfo: hexToBytes(f.parentCoinInfoHex), puzzleHash: ownerPh, amount: 1000n };
const cat = wasm.issueCat(synthKey, [catCoin], 1000n, 0n);
assert.ok(cat.coinSpends.length > 0, "issueCat coinSpends");
assert.equal(cat.assetId.length, 32, "cat assetId 32 bytes");

// --- #35: offer encode/decode roundtrip ---
const offerText = wasm.encodeOffer({ coinSpends: nft.coinSpends, aggregatedSignature: identitySig });
assert.ok(offerText.startsWith("offer1"), "offer text starts with offer1");
const offerBack = wasm.decodeOffer(offerText);
assert.equal(offerBack.coinSpends.length, nft.coinSpends.length, "offer decode roundtrip");

// --- #34: generateItemMetadata + bulkMint ---
const collection = {
  id: "col-1",
  name: "DIG Punks",
  attributes: [{ traitType: "website", value: "https://dig.net" }],
  royaltyPuzzleHash: ownerPh,
  royaltyBasisPoints: 420,
};
const manifest = [0, 1].map((i) => ({
  name: `DIG Punk #${i + 1}`,
  description: "gen",
  attributes: [{ traitType: "Index", value: String(i) }],
  media: {
    dataUris: [`dig://urn:dig:chia:store:root/item${i}.png`],
    dataHash: wasm.sha256(Buffer.from(`bytes-${i}`)),
    metadataUris: [`dig://urn:dig:chia:store:root/item${i}.json`],
    metadataHash: wasm.sha256(Buffer.from(`meta-${i}`)),
    licenseUris: [],
  },
}));
const docs = wasm.generateItemMetadata(collection, manifest);
assert.equal(docs.length, 2, "two item docs");
assert.equal(docs[0].seriesNumber, 1n, "series number 1-based");
assert.equal(docs[1].seriesTotal, 2n, "series total");
assert.equal(docs[0].collection.id, "col-1", "collection block embedded");

// bulkMint needs the DID's coin + proof; use the just-created DID's coin (eve proof).
const didForMint = {
  didCoin: did.didCoin,
  proof: { eveProof: { parentParentCoinInfo: did.didCoin.parentCoinInfo, parentAmount: did.didCoin.amount } },
  launcherId: did.launcherId,
  innerPuzzleHash: did.innerPuzzleHash,
};
const bulk = wasm.bulkMint(synthKey, didForMint, collection, manifest, ownerPh);
assert.ok(bulk.coinSpends.length > 0, "bulkMint coinSpends");
assert.equal(bulk.launcherIds.length, 2, "bulkMint one launcher id per item");
assert.notEqual(
  Buffer.from(bulk.launcherIds[0]).toString("hex"),
  Buffer.from(bulk.launcherIds[1]).toString("hex"),
  "bulk minted items are distinct"
);

// --- #38: mintNftWithDid (single mint authorized by + attributed to a creator DID) ---
const didMint = wasm.mintNftWithDid(synthKey, [coin], didForMint, nftParams, 0n);
assert.ok(didMint.coinSpends.length > 0, "mintNftWithDid coinSpends");
assert.equal(didMint.launcherId.length, 32, "mintNftWithDid launcherId 32 bytes");
// The DID coin must be SPENT in the bundle (it authorizes the attribution), not merely named.
// Compare the raw Coin fields (parent/puzzleHash/amount) — no coinId helper needed.
const sameCoin = (a, b) =>
  Buffer.from(a.parentCoinInfo).equals(Buffer.from(b.parentCoinInfo)) &&
  Buffer.from(a.puzzleHash).equals(Buffer.from(b.puzzleHash)) &&
  BigInt(a.amount) === BigInt(b.amount);
assert.ok(
  didMint.coinSpends.some((cs) => sameCoin(cs.coin, did.didCoin)),
  "mintNftWithDid spends the creator DID coin"
);

// ===========================================================================
// Per-capsule $DIG payment (task #111): mint is FREE of $DIG; a capsule (commit) pays the treasury.
// ===========================================================================

// digConstants: the cross-system DIG asset id + treasury inner puzzle hash.
const digC = wasm.digConstants();
assert.equal(digC.assetId.length, 32, "digConstants.assetId 32 bytes");
assert.equal(
  Buffer.from(digC.assetId).toString("hex"),
  "a406d3a9de984d03c9591c10d917593b434d5263cabe2b42f6b367df16832f81",
  "DIG asset id matches the cross-system contract"
);
assert.equal(
  Buffer.from(digC.treasuryInnerPuzzleHash).toString("hex"),
  "ec7c304708c7d59c078d5ae098d0dea004decf47fa1cafebb266c10ad6466ce8",
  "DIG treasury inner puzzle hash matches the cross-system contract"
);

// A buyer's DIG CAT coin (DIG asset id, owned by ownerPh) for the capsule payment.
const digCat = {
  coin: { parentCoinInfo: hexToBytes(f.parentCoinInfoHex), puzzleHash: hexToBytes("06".repeat(32)), amount: 1000000n },
  info: { assetId: digC.assetId, p2PuzzleHash: ownerPh },
};

// buildDigStorePayment: pay the dynamic per-capsule price (an INPUT amount) to the treasury.
const storeId = mint.newStore.launcherId;
const digPay = wasm.buildDigStorePayment(synthKey, [digCat], storeId, 100000n);
assert.ok(Array.isArray(digPay) && digPay.length > 0, "buildDigStorePayment returns coin spends");

// digTreasuryPaymentCoin: the exact treasury coin the payment emits (CAT-wrapped, NOT the inner ph).
const payCoin = wasm.digTreasuryPaymentCoin(digCat, 100000n);
assert.equal(payCoin.amount, 100000n, "treasury payment coin amount == input");
assert.notEqual(
  Buffer.from(payCoin.puzzleHash).toString("hex"),
  Buffer.from(digC.treasuryInnerPuzzleHash).toString("hex"),
  "treasury payment coin lands at the DIG-CAT-wrapped ph, not the inner ph"
);

// Non-DIG CATs are rejected (only $DIG pays a capsule).
assert.throws(
  () => wasm.buildDigStorePayment(synthKey, [{ ...digCat, info: { assetId: new Uint8Array(32).fill(0xcd), p2PuzzleHash: ownerPh } }], storeId, 1000n),
  "buildDigStorePayment rejects a non-DIG CAT"
);

console.log("All chip35-dl-coin WASM builder checks passed.");
