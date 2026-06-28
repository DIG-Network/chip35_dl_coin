# chip35_dl_coin — Asset toolkit & DataStore delegation design

This document covers the spend/primitive layer this crate adds for the DIG developer-platform
NFT/asset toolkit (roadmap Wave 3: #33, #34, #35, #36) and the **DataStore delegation** builders
that back hub Teams (#43) and revocable deploy tokens (#17). It is the design-of-record for those
features and the contract they expose to the rest of the ecosystem.

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
| `store.rs` | CHIP-0035 DataStore builders (mint/update/melt/oracle/addFee/serialization) PLUS the delegation builders (`admin`/`writer`/`oracle` `delegated_puzzle_from_key`) for hub Teams + revocable deploy tokens. | #43, #17 |
| `metadata.rs` | CHIP-0007 metadata builder + validator. Generates valid CHIP-0007 JSON, computes data/metadata/license hashes from bytes, validates URI↔hash agreement + schema. | #36 |
| `nft.rs` | Single NFT mint (incl. the dig://-capsule media path) + DID-attributed transfer condition. | #33 |
| `collection.rs` | CHIP-0007 collection model + per-item metadata generation from a traits manifest + bulk mint via intermediate launchers. | #34 |
| `did.rs` | DID (creator identity) creation builder. | #35 |
| `cat.rs` | CAT issuance builder. | #35 |
| `offer.rs` | Offer encode/decode (canonical bech32-ish `offer1...` text ↔ spend bundle). | #35 |
| ~~`deploy_token.rs`~~ | **REMOVED** — the bespoke scaffold is superseded. A deploy token is a **revocable writer delegate** (see `store.rs`); there is no separate puzzle. | #17 |

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

## #43 / #17 — DataStore delegation: Teams + revocable deploy tokens (`store.rs`)

> **STATUS: IMPLEMENTED.** The prior `deploy_token.rs` scaffold (a bespoke curried puzzle pending
> security review) is **superseded and removed**. There is no special deploy-token puzzle: the
> CHIP-0035 DataStore delegation layer already provides the exact least-privilege primitive, so we
> simply expose its builders. No new chialisp — `chia-sdk-driver` 0.30 (chip-0035) ships the
> `DelegatedPuzzle` + DataStore delegation layer; this crate exposes the builders that drive it,
> mirroring `DataLayer-Driver`'s shapes byte-for-byte.

**The model.** A DataStore singleton carries a list of `DelegatedPuzzle`s beside its owner. Each
grants one role:

- **Admin** — update the store AND change delegation (add/remove admins/writers). A hub Teams admin.
- **Writer** — create new generations (advance the root = deploy a new capsule) but NOT change
  delegation or transfer ownership. **A revocable deploy token IS a writer delegate** (#17); a hub
  Teams writer (#43).
- **Oracle** — anyone may spend the store for a fixed fee (keyed by a payment puzzle hash, not a
  key — no signer).

**Builders (core/src/store.rs)** — mirror DataLayer-Driver `src/lib.rs`:
```rust
pub fn admin_delegated_puzzle_from_key(synthetic_key: &PublicKey) -> DelegatedPuzzle;  // Admin(curry_tree_hash(key))
pub fn writer_delegated_puzzle_from_key(synthetic_key: &PublicKey) -> DelegatedPuzzle; // Writer(curry_tree_hash(key))
pub fn oracle_delegated_puzzle(oracle_puzzle_hash: Bytes32, oracle_fee: u64) -> DelegatedPuzzle; // Oracle(ph, fee)
```
Admin/Writer curry only the standard puzzle of the synthetic key, so the same key authorizes
whichever role the owner granted it; the role lives in the store's delegated-puzzle set, not the key.

**How delegates are carried (no new spend builders needed — these already existed):**

- **Mint with delegates** — `mint_store(.., delegated_puzzles: Vec<DelegatedPuzzle>, ..)`: launch a
  store with admins/writers/oracle from the start.
- **Add/remove delegates** — `update_store_ownership(datastore, new_owner_ph, new_delegated_puzzles,
  inner_spend)`: replace the delegated-puzzle set. This is the Teams **add-member** and the deploy-
  token **issue + revoke** operation. Authorizable by `Owner` (full) or `Admin` (delegation change
  via `UpdateDataStoreMerkleRoot`); a `Writer` is rejected (`Error::Permission`).
- **Delegate-signed root advance (deploy)** — `update_store_metadata(datastore, new_root_hash, ..,
  inner_spend)` with `DataStoreInnerSpend::Writer(deploy_key)` (or `Admin`): the delegate advances
  the root **without the owner seed**. A root advance IS a metadata update; the `WriterLayer` is the
  metadata-only filter. This is the deploy-token / team-member commit.

**`DataStoreInnerSpend`** selects the authorizing role for an update: `Owner(pk)` /`Admin(pk)`/
`Writer(pk)`. The permission matrix is enforced in `update_store_with_conditions`:

| Role | advance root (metadata) | change delegation / ownership |
|---|---|---|
| Owner | ✅ | ✅ |
| Admin | ✅ | ✅ delegation (not outright transfer) |
| Writer | ✅ | ❌ `Error::Permission` |

**Deploy-token lifecycle (#17)** = the writer-delegate lifecycle:
1. **Issue** — owner adds `writer_delegated_puzzle_from_key(ci_deploy_key)` via `update_store_ownership`.
2. **Deploy** — CI deploy key calls `update_store_metadata(.., Writer(ci_deploy_key))` (no owner seed).
3. **Revoke** — owner/admin replaces the delegated set, dropping that writer; the key can no longer advance.

**Future work (the only non-native extra):** an on-chain **DIG spend cap** bounding cumulative spend
per deploy token (a curried max + counter coin/announced total). Native CHIP-0035 delegation gives
store-binding (the puzzle is part of one store's set), least privilege (writer = metadata-only), and
owner revocation for free; a spend cap is the one thing the native layer does not enforce and is
deferred. It does NOT block Teams or deploy tokens — those ship on the native primitive above.

**wasm exports** — `adminDelegatedPuzzleFromKey(syntheticKey) -> DelegatedPuzzle`,
`writerDelegatedPuzzleFromKey(syntheticKey) -> DelegatedPuzzle`,
`oracleDelegatedPuzzle(oraclePuzzleHash, oracleFee) -> DelegatedPuzzle`. The returned `DelegatedPuzzle`
drops straight into `mintStore`'s `delegatedPuzzles` and `updateStoreOwnership`'s
`newDelegatedPuzzles`. The delegate-signed root advance uses the existing `updateStoreMetadata` with
the `writerPublicKey` (or `adminPublicKey`) argument.

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

### Delegation (#43 Teams / #17 deploy tokens) wiring

The new delegation exports — `adminDelegatedPuzzleFromKey`, `writerDelegatedPuzzleFromKey`,
`oracleDelegatedPuzzle` — plus the already-delegation-aware `mintStore` / `updateStoreOwnership` /
`updateStoreMetadata` are what the next wave consumes:

- **hub.dig.net Teams (#43)** (`apps/web/lib/driver.js`): to add a team member, derive their
  delegate with `writerDelegatedPuzzleFromKey` (writer) or `adminDelegatedPuzzleFromKey` (admin),
  append it to the store's delegated-puzzle set, and call `updateStoreOwnership(store, ownerPh,
  newDelegatedPuzzles, ownerPublicKey)` (signed by the owner, or `adminPublicKey` for an admin
  managing the set). Remove a member by replacing the set without their delegate.
- **digstore deploy tokens (#17)** (CI auto-deploy, roadmap #7/#23): the owner issues a deploy token
  by adding `writerDelegatedPuzzleFromKey(ciDeployKey)` via `updateStoreOwnership`. CI then advances
  the root with `updateStoreMetadata(store, newRoot, .., writerPublicKey=ciDeployKey)` — no owner
  seed in CI. Revoke by replacing the delegated set. (The §21 remote's first-push publisher key is
  the natural deploy key to authorize.)
- **dig-sdk** (`/spend` subpath) re-exports the three new delegation builders alongside the asset
  exports so integrators wire Teams/deploy-tokens without re-implementing the spend layer.
