# chip35_dl_coin — Asset toolkit & deploy-token design

This document covers the spend/primitive layer this crate adds for the DIG developer-platform
NFT/asset toolkit (roadmap Wave 3: #33, #34, #35, #36) and the **scaffold** for scoped deploy
tokens (#17). It is the design-of-record for those features and the contract they expose to the
rest of the ecosystem.

> **Contract reminder (SYSTEM.md / CLAUDE.md):** `chip35_dl_coin` is the *canonical* place to
> construct spend bundles. On-chain spend types live here; a new
> `@dignetwork/chip35-dl-coin-wasm` is released **first**, then the hub frontend and `dig-sdk`
> bump to it and wire it through. The hub must never hand-roll spends. Nothing in this crate does
> networking, signing, or key derivation — it builds keyless coin spends; the caller (Sage /
> `window.chia` / the DIG Browser native wallet) signs and broadcasts.

The crate rests on `chia-sdk-driver` 0.30 (chip-0035 + action-layer features). NFT/DID/CAT/offer
primitives all live in that dependency; this crate's job is to assemble them into the exact spend
builders the toolkit needs and to expose them at the wasm boundary with a stable, typed JS shape.

---

## Module layout (core/src)

| Module | Purpose | Roadmap |
|---|---|---|
| `store.rs` | Existing CHIP-0035 DataStore builders (mint/update/melt/oracle/addFee/serialization). Unchanged. | — |
| `metadata.rs` | CHIP-0007 metadata builder + validator. Generates valid CHIP-0007 JSON, computes data/metadata/license hashes from bytes, validates URI↔hash agreement + schema. | #36 |
| `nft.rs` | Single NFT mint (incl. the dig://-capsule media path) + DID-attributed transfer condition. | #33 |
| `collection.rs` | CHIP-0007 collection model + per-item metadata generation from a traits manifest + bulk mint via intermediate launchers. | #34 |
| `did.rs` | DID (creator identity) creation builder. | #35 |
| `cat.rs` | CAT issuance builder. | #35 |
| `offer.rs` | Offer encode/decode (canonical bech32-ish `offer1...` text ↔ spend bundle). | #35 |
| `deploy_token.rs` | **SCAFFOLD** — store-bound, spend-capped, revocable deploy-token delegation puzzle. Design + interface + failing tests only; on-chain auth pending review. | #17 |

The wasm crate (`wasm/src`) adds one thin `#[wasm_bindgen]` wrapper per new public core function,
reusing the existing serde boundary helpers in `wasm/src/types.rs`.

---

## #36 — CHIP-0007 metadata builder + validator (`metadata.rs`)

CHIP-0007 is the off-chain NFT metadata JSON standard (the document an NFT's `metadata_uris` point
at). Today every consumer (hub badge minting, MintGarden adapter) hand-computes SHA-256 over the
bytes and trusts raw input. This module is the single shared, tested implementation.

**`Chip0007Metadata`** — the typed model of the CHIP-0007 document:
`format` (`"CHIP-0007"`), `name`, `description`, `sensitive_content` (bool or list),
`collection` (id/name/attributes), `attributes` (trait_type/value), `minting_tool`, `series_number`,
`series_total`. Serializes to canonical JSON with `serde_json` (stable key order via an explicit
field order; CHIP-0007 does not mandate sorting but we emit deterministic output so the hash is
reproducible).

**Hashing.** `sha256(bytes) -> Bytes32`. The on-chain NFT metadata pins three hashes:
- `data_hash` = sha256(the media bytes),
- `metadata_hash` = sha256(the CHIP-0007 JSON bytes),
- `license_hash` = sha256(the license bytes).

`compute_metadata_hash(&self) -> Bytes32` serializes the document and hashes it, so a caller never
hand-rolls the hash.

**Validation** — `validate(...)` checks:
1. `format == "CHIP-0007"`.
2. Required fields present (`name`).
3. For each `(uris, hash)` pair the caller provides actual bytes for, `sha256(bytes) == hash`
   (URI↔hash agreement). This is the footgun-closing check: the on-chain hash MUST match what the
   URI actually serves.
4. `series_number <= series_total` when both present.

Errors are a dedicated `MetadataError` so JS gets actionable messages.

**Wasm:** `buildChip0007Metadata(jsonValue) -> { json, metadataHash }`, `sha256Hex(bytes) -> hex`,
`validateChip0007(jsonValue, {dataBytes?, metadataBytes?, licenseBytes?, dataHash?, ...}) -> {ok, errors[]}`.

---

## #33 — NFT media in a DIG capsule (`nft.rs`)

The killer differentiator: an NFT whose media + metadata live in a DIG capsule, addressed by a
`dig://` URN, with an https gateway fallback URI, and hashes computed from the real bytes (via #36).
This crate does **not** build the capsule (that is `digstore`); it exposes the spend builder that
takes the already-computed hashes + the dig:// URIs and mints the NFT.

**`mint_nft`** parameters: minter synthetic key, selected coins, the NFT `data_uris`
(`["dig://urn:dig:...", "https://gateway.../..."]` — dig:// first, https fallback second),
`data_hash`, `metadata_uris` + `metadata_hash`, `license_uris` + `license_hash`,
`edition_number`/`edition_total`, `royalty_puzzle_hash`, `royalty_basis_points`,
`p2_puzzle_hash` (recipient), optional `did` attribution (launcher id + DID inner puzzle hash →
`TransferNft`), and `fee`. It builds the standard NFT mint via `Launcher::mint_nft` and returns the
coin spends + the resulting `Nft` summary (launcher id, coin, royalty).

The dig://-vs-https ordering is a convention, not enforced on-chain: the builder accepts whatever
URI list the caller passes. The toolkit/CLI is responsible for putting the dig:// URN first.

---

## #34 — Collection primitive (`collection.rs`)

Creators think in *collections*, not individual mints. This models a CHIP-0007 collection and bulk
mints from a traits manifest.

**`Collection`** — `id` (deterministic from creator DID + name, or caller-supplied), `name`,
`attributes` (icon/banner/website/twitter/etc as CHIP-0007 collection attributes), shared
`royalty_puzzle_hash` + `royalty_basis_points`, optional `did` (launcher id) for attribution, and a
`license_uris`/`license_hash` shared across items.

**`generate_item_metadata(collection, manifest)`** — accepts a *parsed* traits manifest (Vec of
items, each with name/description/attributes/data+metadata+license uris+hashes — JSON in, no file
IO) and produces one `Chip0007Metadata` per item with the collection block + `series_number`/
`series_total` filled in. The toolkit (digstore/CLI) parses CSV/JSON into this manifest; this crate
only consumes the parsed form (the task is explicit: accept parsed JSON, not file IO).

**`build_bulk_mint`** — bulk-mints N NFTs in one bundle using `IntermediateLauncher` (mint_number,
mint_total) → `Launcher::mint_nft`, with shared royalty + collection + optional DID attribution,
spending the DID once to authorize all mints (`did.update(... mint_1.extend(mint_2)...)`), matching
the canonical bulk-mint pattern in chia-sdk-driver's own `test_bulk_mint`. Returns the coin spends +
the list of resulting `Nft`s.

---

## #35 — Reachability via wasm (`did.rs`, `cat.rs`, `offer.rs` + wrappers)

Every builder must be callable from JS so the asset SDK + CLI can wrap them.

- **`create_did`** — creates a DID (creator identity) singleton from the minter's coins. Wasm:
  `createDid(...) -> {coinSpends, did}`.
- **`issue_cat`** — single-issuance (genesis-by-coin-id) CAT mint. Wasm: `issueCat(...) -> {coinSpends, assetId, cats}`.
- **`encode_offer` / `decode_offer`** — canonical offer-text ↔ spend-bundle, via chia-sdk-driver's
  compression. Wasm: `encodeOffer(spendBundle) -> string`, `decodeOffer(text) -> {coinSpends, aggregatedSignature}`.
- Existing store builders + `mintNft` + `bulkMint` + `createDid` + `issueCat` + offer codec are the
  full asset surface the SDK/CLI wraps.

Full *offer construction* (build an offer that swaps asset A for B with royalties) needs trade
managers and a richer coin-selection model than the keyless boundary cleanly supports; v1 ships the
offer **codec** (encode/decode) reachable from wasm and defers offer *construction* to a later wave
(noted below). Taking an offer is a settlement spend the wallet (Sage) already performs.

---

## #17 — Scoped deploy-token delegation puzzle (SCAFFOLD — pending review)

> **STATUS: SCAFFOLD.** This is a security-critical on-chain authorization puzzle. The design,
> Rust interface, and failing tests below describe the intended behavior. The CLVM puzzle itself
> and the spend-builder body are intentionally **not** implemented here — they require security
> review before any real authorization code is written. The tests are `#[ignore]`d with a clear
> "scaffold pending review" reason so they document intent without gating CI green.

**Problem.** Auto-deploy from CI (roadmap #7) needs to advance a store's on-chain root **without**
shipping the funded master seed into CI. A leaked seed = total loss of the store *and* the wallet.
We need a credential that is (a) bound to ONE store, (b) capped in how much DIG it can spend, (c)
expirable, and (d) revocable by the owner — least privilege for a deploy bot.

**Model.** A *deploy token* is a delegated authority curried into a puzzle that the store owner
authorizes once (on-chain) and a CI key then uses to sign root-advance spends. It is conceptually a
constrained sibling of the existing `DelegatedPuzzle::Writer` (writers can already update metadata),
hardened with three on-chain-enforced limits:

1. **Store binding.** The puzzle is curried with the store `launcher_id`; it can only authorize a
   spend of that singleton. A token for store A cannot touch store B.
2. **Spend cap.** Curried with a max cumulative DIG amount (and/or a max number of root advances).
   Each advance asserts the running total stays under the cap. (Cap accounting across advances needs
   a small state coin or an announced counter — design open, flagged for review.)
3. **Expiry.** Curried with an `ASSERT_BEFORE_SECONDS_ABSOLUTE` so the token self-expires.
4. **Revocation.** The owner can melt/replace the delegation (re-key the store's delegated-puzzle
   set), invalidating the token immediately — reusing the existing `update_store_ownership`
   delegated-puzzle replacement path.

**Interface (scaffold).**
```rust
pub struct DeployTokenTerms {
    pub store_launcher_id: Bytes32,
    pub authorized_public_key: PublicKey, // the CI deploy key
    pub max_total_mojos: u64,             // spend cap
    pub max_advances: u32,                // optional advance count cap
    pub expires_at_seconds: u64,          // absolute unix seconds
}

/// SCAFFOLD: returns the delegated-puzzle entry the owner adds to the store so the deploy key can
/// advance the root within the curried limits. NOT YET IMPLEMENTED — pending security review.
pub fn build_deploy_token(terms: &DeployTokenTerms) -> Result<DelegatedPuzzle, WalletError>;

/// SCAFFOLD: build the root-advance spend signed by the deploy key, asserting the cap/expiry.
/// NOT YET IMPLEMENTED — pending security review.
pub fn deploy_token_advance_root(
    datastore: DataStore,
    new_root_hash: Bytes32,
    terms: &DeployTokenTerms,
) -> Result<SuccessResponse, WalletError>;
```

**Open questions for review (do not implement before resolving):**
- Cap accounting: stateful (counter coin) vs. stateless (per-spend cap only)? A pure per-spend cap
  is simpler and revocable but does not bound *cumulative* spend; a counter coin bounds cumulative
  but adds a coin to every advance. Recommend starting stateless (per-advance cap + expiry +
  revocation) and adding cumulative accounting only if required.
- Does the deploy token reuse the writer-filter puzzle (metadata-only, which is exactly a root
  advance) curried with the extra asserts, or a new puzzle? Reusing writer-filter keeps the audited
  surface small.
- Revocation UX: melt-and-recreate the delegated set vs. a dedicated revocation list.

Until reviewed, the wasm boundary exposes **no** deploy-token functions — the scaffold is core-only
so nothing downstream can accidentally depend on unreviewed auth code.

---

## Cross-module wiring (what the next wave consumes)

Once a new `@dignetwork/chip35-dl-coin-wasm` is cut (human-gated):
- **dig-sdk** (`/spend` subpath) re-exports the new wasm functions: `mintNft`, `bulkMint`,
  `createDid`, `issueCat`, `encodeOffer`, `decodeOffer`, `buildChip0007Metadata`, `sha256Hex`,
  `validateChip0007`.
- **hub.dig.net** (`apps/web/lib/driver.js`) bumps the dep and wires the NFT studio (#37) + badge
  minting onto `mintNft`/`bulkMint` + `buildChip0007Metadata` (replacing the hand-computed badge
  metadata).
- **digstore** CLI (`digstore nft|collection|did|offer`, #35) and the asset SDK wrap the same
  exports; digstore writes the capsule + computes the byte hashes, then calls `mintNft` with the
  dig:// URN + computed hashes.
