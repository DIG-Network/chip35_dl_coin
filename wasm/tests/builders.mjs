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

// #189 (emit-side twin of digstore's #187): a collection-level attribute passed as the current
// hub camelCase shape `{ traitType, value }` must still be ACCEPTED (back-compat, alias), but the
// canonical JSON it produces must render `"type"` (CHIP-0007-conformant) — never `"trait_type"` —
// for the collection attribute, while the item-level attribute stays `"trait_type"`.
const builtWithCollection = wasm.buildChip0007Metadata({
  name: "DIG Punk #1",
  collection: {
    id: "col-1",
    name: "DIG Punks",
    attributes: [{ traitType: "icon", value: "https://dig.net/icon.png" }],
  },
  attributes: [{ traitType: "Background", value: "Blue" }],
});
assert.ok(
  builtWithCollection.json.includes('"collection":{"id":"col-1","name":"DIG Punks","attributes":[{"type":"icon","value":"https://dig.net/icon.png"}]}'),
  `collection attribute must serialize as "type", got: ${builtWithCollection.json}`
);
assert.ok(
  builtWithCollection.json.includes('"attributes":[{"trait_type":"Background","value":"Blue"}]'),
  `item attribute must still serialize as "trait_type", got: ${builtWithCollection.json}`
);
assert.ok(
  !builtWithCollection.json.includes('"trait_type":"icon"'),
  `collection attribute must NOT serialize as "trait_type", got: ${builtWithCollection.json}`
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

// --- #221: bulkMintFunded (multi-item mint funded by a separate XCH coin) ---
// A separate XCH coin donates 1 mojo/item to the bundle (the DID's own value can't); excess returns
// as change. The funded variant adds exactly one extra coin spend (the funding coin) over bulkMint.
const fundingCoin = { parentCoinInfo: ownerPh, puzzleHash: ownerPh, amount: 100n };
const bulkFunded = wasm.bulkMintFunded(synthKey, didForMint, collection, manifest, ownerPh, fundingCoin, synthKey);
assert.equal(bulkFunded.launcherIds.length, 2, "bulkMintFunded one launcher id per item");
assert.equal(
  bulkFunded.coinSpends.length,
  bulk.coinSpends.length + 1,
  "bulkMintFunded adds exactly the funding-coin spend"
);

// --- #1132: bulkMintFundedNoDid (multi-item mint funded by a COIN SET, single fee, NO DID) ---
// No DID coin: the coin set funds `items.length` launcher mojos + the fee; excess returns as change.
// Every selected coin must be spent exactly once (the #304 bug loops a single-mint builder over ONE
// coin, double-spending it); assert no coin id repeats across the returned spends.
const noDidCoins = [
  { parentCoinInfo: ownerPh, puzzleHash: ownerPh, amount: 50n },
  { parentCoinInfo: ownerPh, puzzleHash: ownerPh, amount: 30n },
];
const bulkNoDid = wasm.bulkMintFundedNoDid(synthKey, collection, manifest, ownerPh, noDidCoins, 5n);
assert.equal(bulkNoDid.launcherIds.length, 2, "bulkMintFundedNoDid one launcher id per item");
const noDidSpentCoins = bulkNoDid.coinSpends.map((cs) =>
  `${Buffer.from(cs.coin.parentCoinInfo).toString("hex")}:${Buffer.from(cs.coin.puzzleHash).toString("hex")}:${BigInt(cs.coin.amount)}`
);
assert.equal(
  new Set(noDidSpentCoins).size,
  noDidSpentCoins.length,
  "bulkMintFundedNoDid spends no coin more than once (no double-spend)"
);
// Underfunded coin set (total < items.length + fee) throws a PARSE_ERROR.
assert.throws(
  () => wasm.bulkMintFundedNoDid(synthKey, collection, manifest, ownerPh, [{ parentCoinInfo: ownerPh, puzzleHash: ownerPh, amount: 1n }], 0n),
  (e) => e.code === "PARSE_ERROR",
  "bulkMintFundedNoDid rejects an underfunded coin set"
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

// ===========================================================================
// Trustless lazy mint / mint-on-claim (roadmap #40): the creator DID precommits a collection ONCE,
// then ANYONE claims an individual NFT on demand (no further DID involvement).
// ===========================================================================

const lazyItems = [0, 1, 2].map((i) => ({
  metadata: {
    dataUris: [`dig://urn:dig:chia:store:root/lazy${i}.png`],
    dataHash: wasm.sha256(Buffer.from(`lazy-bytes-${i}`)),
    metadataUris: [`dig://urn:dig:chia:store:root/lazy${i}.json`],
    metadataHash: wasm.sha256(Buffer.from(`lazy-meta-${i}`)),
    licenseUris: [],
    editionNumber: 1n,
    editionTotal: 1n,
  },
  royaltyBasisPoints: 300,
}));

// COMMIT: the creator DID spends once to precommit the 3-item collection (free / direct mint).
const lazyCommit = wasm.buildLazyMintCommit(
  synthKey,
  didForMint,
  collection,
  lazyItems,
  { directMint: true },
  undefined
);
assert.ok(lazyCommit.coinSpends.length > 0, "lazy commit emits the DID spend");
assert.equal(lazyCommit.launcherIds.length, 3, "one precomputed launcher id per item");
assert.equal(lazyCommit.commitCoins.length, 3, "one commitment coin per item");
assert.equal(typeof lazyCommit.descriptor, "string", "descriptor is the opaque JSON handle");
assert.equal(lazyCommit.root.length, 32, "commit root (DID coin id) is 32 bytes");
assert.notEqual(
  Buffer.from(lazyCommit.launcherIds[0]).toString("hex"),
  Buffer.from(lazyCommit.launcherIds[1]).toString("hex"),
  "precommitted launcher ids are distinct per item"
);

// determinism: an identical commit (same DID coin) yields the same precomputed launcher ids.
const lazyCommit2 = wasm.buildLazyMintCommit(
  synthKey, didForMint, collection, lazyItems, { directMint: true }, undefined
);
assert.equal(
  Buffer.from(lazyCommit2.launcherIds[1]).toString("hex"),
  Buffer.from(lazyCommit.launcherIds[1]).toString("hex"),
  "precomputed launcher ids are deterministic"
);

// CLAIM: a different party unrolls + mints item 1, funding the mojo from their own coin.
const claimerCoin = {
  parentCoinInfo: hexToBytes(f.parentCoinInfoHex),
  puzzleHash: hexToBytes("0b".repeat(32)),
  amount: 5n,
};
const lazyClaim = wasm.buildLazyMintClaim(
  synthKey,
  [claimerCoin],
  hexToBytes("0c".repeat(32)), // claimer recipient puzzle hash
  lazyCommit.descriptor,
  1,
  undefined,
  0n
);
assert.ok(lazyClaim.coinSpends.length > 0, "lazy claim emits unroll+mint coin spends");
assert.equal(lazyClaim.launcherId.length, 32, "lazy claim launcherId 32 bytes");
assert.equal(
  Buffer.from(lazyClaim.launcherId).toString("hex"),
  Buffer.from(lazyCommit.launcherIds[1]).toString("hex"),
  "claim mints exactly the precommitted launcher id for item 1"
);

// out-of-range index is rejected.
assert.throws(
  () => wasm.buildLazyMintClaim(synthKey, [claimerCoin], hexToBytes("0c".repeat(32)), lazyCommit.descriptor, 9, undefined, 0n),
  "lazy claim rejects an out-of-range item index"
);

// a payment-gated policy is accepted at the boundary (enforcement deferred — see DESIGN.md #40).
const lazyCommitPaid = wasm.buildLazyMintCommit(
  synthKey, didForMint, collection, lazyItems,
  { paymentGated: { price: 1000n, asset: { xch: true }, payee: ownerPh } },
  undefined
);
assert.ok(lazyCommitPaid.coinSpends.length > 0, "payment-gated lazy commit builds");

// ALLOWLIST GATE (#107): an allowlist-gated commit + claim. The proof is ENFORCED off-chain by
// buildLazyMintClaim; verifyMerkleMembership checks one without building a spend.
const claimerPh = hexToBytes("0c".repeat(32)); // the claimer recipient puzzle hash used above
// A single-member allowlist: the merkle root of one leaf is sha256(0x01 || leaf); its proof is
// { path: 0, proof: [] } (no siblings). Computed here without re-implementing tree building.
const singleLeafRoot = wasm.sha256(Buffer.concat([Buffer.from([0x01]), Buffer.from(claimerPh)]));
const memberProof = { path: 0, proof: [] };

// verifyMerkleMembership: a valid single-leaf proof verifies; a wrong root does not.
assert.equal(typeof wasm.verifyMerkleMembership(claimerPh, memberProof, singleLeafRoot), "boolean");
assert.ok(
  wasm.verifyMerkleMembership(claimerPh, memberProof, singleLeafRoot),
  "valid membership proof verifies against the single-leaf root"
);
assert.ok(
  !wasm.verifyMerkleMembership(hexToBytes("0d".repeat(32)), memberProof, singleLeafRoot),
  "a non-member leaf does not verify"
);

// A gated commit, then: a claim with NO proof is rejected (ALLOWLIST_DENIED); a claim WITH the valid
// proof for the claimer's own address mints the precommitted launcher id.
const lazyCommitGated = wasm.buildLazyMintCommit(
  synthKey, didForMint, collection, lazyItems, { directMint: true }, singleLeafRoot
);
assert.throws(
  () => wasm.buildLazyMintClaim(synthKey, [claimerCoin], claimerPh, lazyCommitGated.descriptor, 0, undefined, 0n),
  (e) => e && e.code === "ALLOWLIST_DENIED",
  "a gated claim with no proof throws ALLOWLIST_DENIED"
);
const gatedClaim = wasm.buildLazyMintClaim(
  synthKey, [claimerCoin], claimerPh, lazyCommitGated.descriptor, 0, memberProof, 0n
);
assert.equal(
  Buffer.from(gatedClaim.launcherId).toString("hex"),
  Buffer.from(lazyCommitGated.launcherIds[0]).toString("hex"),
  "a gated claim with a valid proof mints the precommitted launcher id"
);

// lazyMint exports are advertised in capabilities().
const caps = wasm.capabilities();
assert.ok(caps.builders.includes("buildLazyMintCommit"), "buildLazyMintCommit advertised");
assert.ok(caps.builders.includes("buildLazyMintClaim"), "buildLazyMintClaim advertised");
assert.ok(caps.builders.includes("verifyMerkleMembership"), "verifyMerkleMembership advertised");
assert.ok(caps.errorCodes.includes("ALLOWLIST_DENIED"), "ALLOWLIST_DENIED error code advertised");

// ===========================================================================
// Shared coin selection + consolidation (epic #410 / #413)
// ===========================================================================

const mkCoin = (parent, amount) => ({
  parentCoinInfo: hexToBytes(String(parent).padStart(2, "0").repeat(32)),
  puzzleHash: ownerPh,
  amount: BigInt(amount),
});
const xchAsset = { xch: true };

// selectCoins — Ok: largest-first, correct total/change/coinCount, asset echoed.
const selOk = wasm.selectCoins(
  [mkCoin(1, 5), mkCoin(2, 30), mkCoin(3, 10), mkCoin(4, 20)],
  45n,
  xchAsset,
  undefined
);
assert.equal(selOk.ok, true, "selectCoins Ok");
assert.equal(selOk.coins.length, 2, "selectCoins picks two coins");
assert.equal(selOk.coins[0].amount, 30n, "selectCoins returns largest-first");
assert.equal(selOk.coins[1].amount, 20n, "selectCoins second-largest next");
assert.equal(selOk.total, 50n, "selectCoins total");
assert.equal(selOk.change, 5n, "selectCoins change");
assert.equal(selOk.coinCount, 2, "selectCoins coinCount");
assert.equal(selOk.asset.xch, true, "selectCoins echoes the xch asset");

// selectCoins — NeedsConsolidation: enough value, too many coins for the cap.
const selNeeds = wasm.selectCoins(
  [mkCoin(1, 10), mkCoin(2, 10), mkCoin(3, 10), mkCoin(4, 10)],
  40n,
  xchAsset,
  3
);
assert.equal(selNeeds.ok, false, "selectCoins over-cap is not ok");
assert.equal(selNeeds.needsConsolidation, true, "selectCoins signals needsConsolidation");
assert.equal(selNeeds.availableCoinCount, 4, "needsConsolidation availableCoinCount");
assert.equal(selNeeds.availableTotal, 40n, "needsConsolidation availableTotal");
assert.equal(selNeeds.required, 40n, "needsConsolidation required");
assert.equal(selNeeds.cap, 3, "needsConsolidation cap echoed");

// selectCoins — InsufficientFunds: distinct from needsConsolidation.
const selShort = wasm.selectCoins([mkCoin(1, 10), mkCoin(2, 10)], 30n, xchAsset, undefined);
assert.equal(selShort.ok, false, "selectCoins insufficient is not ok");
assert.equal(selShort.needsConsolidation, false, "insufficient funds is NOT needsConsolidation");
assert.equal(selShort.availableTotal, 20n, "insufficient availableTotal");

// selectCoins — echoes a CAT asset id when selecting for a CAT tail.
const selCatAsset = wasm.selectCoins(
  [mkCoin(1, 100)],
  10n,
  { assetId: digC.assetId },
  undefined
);
assert.equal(selCatAsset.ok, true, "selectCoins Ok for a CAT tail");
assert.equal(
  Buffer.from(selCatAsset.asset.assetId).toString("hex"),
  Buffer.from(digC.assetId).toString("hex"),
  "selectCoins echoes the CAT asset id"
);

// buildCoinConsolidation — merges the SMALLEST `cap` coins into ONE output; requires >= 2.
const consol = wasm.buildCoinConsolidation(
  synthKey,
  [mkCoin(1, 100), mkCoin(2, 1), mkCoin(3, 50), mkCoin(4, 2), mkCoin(5, 3)],
  3,
  0n
);
assert.ok(Array.isArray(consol) && consol.length === 3, "consolidation spends the 3 smallest coins");
const consolAmounts = consol.map((cs) => cs.coin.amount).sort((a, b) => Number(a - b));
assert.deepEqual(consolAmounts, [1n, 2n, 3n], "only the smallest 3 coins are merged");
assert.throws(
  () => wasm.buildCoinConsolidation(synthKey, [mkCoin(1, 10)], 50, 0n),
  "buildCoinConsolidation requires at least 2 coins"
);

// buildCatConsolidation — merges >= 2 CAT coins of one asset into one output.
const mkCat = (parent, amount) => ({
  coin: { parentCoinInfo: hexToBytes(String(parent).padStart(2, "0").repeat(32)), puzzleHash: hexToBytes("06".repeat(32)), amount: BigInt(amount) },
  info: { assetId: digC.assetId, p2PuzzleHash: ownerPh },
});
const catConsol = wasm.buildCatConsolidation(synthKey, [mkCat(1, 100), mkCat(2, 5), mkCat(3, 7)], 2);
assert.ok(Array.isArray(catConsol) && catConsol.length > 0, "cat consolidation returns coin spends");
assert.throws(
  () => wasm.buildCatConsolidation(synthKey, [mkCat(1, 10)], 50),
  "buildCatConsolidation requires at least 2 CAT coins"
);
assert.throws(
  () => wasm.buildCatConsolidation(synthKey, [mkCat(1, 10), { ...mkCat(2, 10), info: { assetId: new Uint8Array(32).fill(0xcd), p2PuzzleHash: ownerPh } }], 50),
  "buildCatConsolidation rejects mixed asset ids"
);

// The new exports are advertised in capabilities().
assert.ok(caps.builders.includes("selectCoins"), "selectCoins advertised");
assert.ok(caps.builders.includes("buildCoinConsolidation"), "buildCoinConsolidation advertised");
assert.ok(caps.builders.includes("buildCatConsolidation"), "buildCatConsolidation advertised");

console.log("All chip35-dl-coin WASM builder checks passed.");
