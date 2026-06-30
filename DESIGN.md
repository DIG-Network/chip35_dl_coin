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
| `payment.rs` | In-dapp **payment** (XCH + any CAT incl. DIG) + **paywall** (pay-to-unlock receipt + verify). | #46 |
| `gating.rs` | **NFT-gating** — read/verify NFT ownership + collection membership (no spend). | #46 |
| `subscription.rs` | Subscription / recurring **scaffold** (clear TODO; needs a time-locked/delegated puzzle). | #46 |
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

## #46 — In-dapp monetization: payment, paywall, NFT-gating (`payment.rs`, `gating.rs`, `subscription.rs`)

> **STATUS: IMPLEMENTED** (payment + paywall + NFT-gating). Subscriptions are a **clearly-marked
> scaffold** (not faked). This is the first INBOUND economic path: a dapp deployed on DIG can EARN,
> where before every spend was outbound. The buyer's wallet signs; nothing here signs or networks.

### Payment (`payment.rs`)
A buyer pays the dapp owner a specified amount in **XCH or any CAT (incl. DIG)**, settling to the
owner's puzzle hash. Reuses the existing keyless patterns (standard-layer XCH spend; `Cat::spend_all`
ring spend for CATs — the same primitives `cat.rs`/`store.rs` use).

```rust
pub fn build_xch_payment(buyer_synthetic_key, selected_coins, owner_puzzle_hash, amount, nonce, fee)
    -> Result<PaymentResponse>;   // -> { coin_spends, receipt }
pub fn build_cat_payment(buyer_synthetic_key, selected_cats: Vec<Cat>, owner_puzzle_hash, amount, nonce)
    -> Result<PaymentResponse>;   // CAT ring nets to zero; carry an XCH fee with a separate coin via add_fee
pub fn payment_nonce(request_bytes) -> Bytes32;   // sha256(dappId||resource||user) — deterministic unlock nonce
```

The owner payment coin carries memos `[owner_puzzle_hash, nonce]`: the first hints the coin to the
owner (so their wallet sees it), the second is the paywall match key. The `nonce` ties one off-chain
unlock request to one on-chain coin → replay-proof without any new puzzle (it is just an indexed
CREATE_COIN memo).

### Paywall — pay-to-unlock (`payment.rs`)
A payment + a verifiable receipt the dapp/SDK checks once the spend confirms.

```rust
pub fn verify_payment_receipt(observed: &ObservedPayment, owner_puzzle_hash, min_amount,
    asset: PaymentAsset, require_nonce: Option<Bytes32>) -> Result<(), PaywallError>;
```

The dapp issues a nonce → buyer pays via `build_*_payment` → after confirm the dapp reads the owner's
coin (by the receipt's coin id, or by scanning the owner's hinted coins for the nonce), fills in an
`ObservedPayment`, and calls `verify_payment_receipt`. `Ok(())` = grant; `Err(PaywallError)` explains
exactly which gate failed (wrong recipient / underpaid / wrong asset / nonce mismatch). Pass
`require_nonce = Some(..)` (recommended) to bind the unlock to a specific request and defeat replay.

### NFT-gating (`gating.rs`) — read/verify, NOT a spend
Prove ownership of an NFT / collection membership so a dapp can gate on it.

```rust
pub fn read_nft_ownership(parent_spend) -> Result<NftOwnershipProof, GatingError>;
pub fn prove_nft_ownership(parent_spend, claimed_owner_ph, required_nft: Option<Bytes32>) -> Result<..>;
pub fn prove_collection_membership(parent_spend, claimed_owner_ph, required_did) -> Result<..>;
```

`parent_spend` is the coin spend that created the NFT's CURRENT coin (the caller fetches it from a
full node). `Nft::parse_child` reconstructs the latest `NftInfo`; the proof exposes `launcher_id`,
`owner_puzzle_hash` (= the holder's address inner p2 hash), `attributed_did` (= `current_owner`, the
creator/collection DID), and `nft_coin_id`. A dapp requires `owner == connected wallet` and
optionally `attributed_did == required collection DID` / `launcher_id == a specific NFT`. The caller
pairs the read facts with a coinset liveness check (is `nft_coin_id` still unspent) before granting.

### Subscriptions — scaffold only (`subscription.rs`)
A recurring/subscription model is **not** just a spend builder: Chia has no native recurring payment,
so it needs a time-locked / pre-authorized **delegated-spend puzzle** (chialisp + a security review,
the same bar that retired the hand-rolled `deploy_token`). `build_subscription_authorization` /
`build_subscription_claim` exist with the intended `SubscriptionTerms` shape but return
`Error::Parse("not yet implemented…")` — they do not fake a spend. **Until the puzzle ships, model
recurring billing as a fresh one-shot payment per period** (buyer approves each renewal; paywall gates
on the latest period's receipt).

### DID-attributed mint (verify for #38)
The DID-attributed mint primitive **already exists**: `create_did` (creator identity singleton) +
`mint_nft`/`build_bulk_mint` with `DidAttribution { launcher_id, inner_puzzle_hash }` set the on-chain
`TransferNft` condition attributing the NFT to the DID. #38's remaining work is *auto-composing the
DID's acknowledging spend into the mint bundle* end-to-end (a "mint as your creator identity" toggle)
— the building blocks are here; the orchestration is the #38 task.

### wasm exports (#46)
`buildPayment(buyerKey, selectedCoins, ownerPuzzleHash, amount, nonce, fee) -> { coinSpends, receipt }`,
`buildCatPayment(buyerKey, selectedCats, ownerPuzzleHash, amount, nonce) -> { coinSpends, receipt }`,
`paymentNonce(requestBytes) -> Uint8Array`,
`verifyPaymentReceipt(observed, ownerPuzzleHash, minAmount, requiredAsset, requireNonce?) -> { ok, error? }`,
`proveNftOwnership(parentSpend, claimedOwnerPuzzleHash, requiredNft?) -> { ok, proof?, error? }`,
`proveCollectionMembership(parentSpend, claimedOwnerPuzzleHash, requiredDid) -> { ok, proof?, error? }`,
`readNftOwnership(parentSpend) -> { ok, proof?, error? }`. `PaymentAsset` is `{ xch:true }` or
`{ assetId }`; a `Cat` is `{ coin, lineageProof?, info:{ assetId, hiddenPuzzleHash?, p2PuzzleHash } }`
(exactly `chip0002_getAssetCoins`'s shape).

---

## #40 — Trustless lazy mint (mint-on-claim) (`lazy_mint.rs`)

> **STATUS: IMPLEMENTED + SIMULATOR-VALIDATED** (commit + direct/free claim, provenance by lineage).
> Allowlist (merkle) gating is now **ENFORCED OFF-CHAIN** at the keyless builder boundary (a gated
> claim requires a valid membership proof for the claimer's own puzzle hash — simulator-validated);
> **trustless ON-CHAIN** allowlist enforcement and **payment-gated** atomic enforcement remain DEFERRED
> with precise blockers (see "Validated vs deferred" below). This is the primitive that lets the hub
> drop studio offer self-serve on-chain drops it honestly blocks today (`drop-model.js`
> `LAZY_MINT_DEFERRED` / `ALLOWLIST_ONCHAIN_DEFERRED`).

**Blueprint + attribution.** Ported from mintgarden-io/secure-the-mint (Apache-2.0, © 2024 Andreas
Greimel; itself based on Chia-Network's `secure_the_bag`, pre-launcher idea credited to trepca). The
Apache license is vendored at `puzzles/LICENSE-APACHE`; the project `NOTICE` lists the derived files;
the ported `.clsp` carry in-file attribution.

### Design decision — SDK primitives, NOT custom compiled puzzles (for the validated path)

secure-the-mint's custom `secure_the_mint_launcher.clsp` exists to solve two problems: (1) the Chia
singleton launcher is insecure (its eve spend can be solved with any puzzle hash), so the launcher's
creator must assert the eve commitment; and (2) at commit time you do not know the launcher coin id
(it depends on the whole tree path), so that assertion has to be **recomputed on-chain** from the
pre-launcher's own `my_id`.

In `chia-sdk-driver` 0.30 BOTH problems already have a clean, audited solution and **neither needs a
custom puzzle**:

- **Launcher id is known at build time.** `Launcher::new(parent_coin_id, 1)` fully determines the
  launcher coin from `(parent_coin_id, SINGLETON_LAUNCHER_HASH, 1)`, so `launcher.coin().coin_id()`
  (= the NFT launcher id) is computable the instant the parent coin is fixed — which, in the commit,
  is the moment the creator picks the DID coin to spend. No on-chain recomputation needed.
- **The eve commitment is asserted by the SDK.** `Launcher::spend` already returns the
  `assert_coin_announcement(...)` that forces the launcher's eve spend to commit to the exact NFT
  puzzle hash — the same security property the custom pre-launcher hand-rolled, but provided (and
  test-covered) by the SDK.
- **A fixed "unroll node" is a quoted-conditions coin.** `ctx.delegated_spend(conditions)` /
  `clvm_quote!` produce a `(q . conditions)` coin spendable by anyone with an empty solution — exactly
  a secure-the-bag node — and `Conditions::create_coin(fixed_ph, amount, memos)` forces a fixed child.
  `chia_sdk_types::puzzles::P2CurriedArgs` is the audited "commit to a fixed puzzle hash now, reveal +
  run it later" wrapper if a commitment hash (rather than the revealed node) must be what the parent
  creates.

Re-implementing the pre-launcher in clsp would mean re-deriving the NFT layer puzzle hashes by hand
(`puzzle-hash-of-curried-function` over the singleton / state / ownership layers) and keeping that in
lockstep with `chia-puzzles` forever — a large, fragile surface for ZERO behavioural gain over the SDK
layer hashing. So the validated builders compose `Launcher` + `NftMint` (`transfer_condition: None`) +
quoted unroll coins. **The ported `.clsp` are kept in `puzzles/` as the auditable reference** of the
secure-the-mint mechanism we mirror (same role as `delegation_layer.clsp`/`writer_filter.clsp`, which
also ship inside `chia-sdk-driver`), and to keep the door open for a future clsp-backed payment/
allowlist enforcement; the Rust builders do not load them at runtime.

### Coin layout (what we build)

A **flat secure-the-bag**, chosen over the deep N-ary tree because the offline keyless boundary returns
one set of coin spends per call and the hub's drops are tens–hundreds of items (a deep tree's value-
left-on-the-table multi-block unroll needs full-node coin selection the keyless builder can't express):

- **Commit (`build_lazy_mint_commit`)** — the creator DID is spent **once**. For each of the N items it
  emits `CREATE_COIN(commit_ph_i, 0, hint=launcher_id_i)` where `commit_ph_i =
  P2CurriedArgs::new(launcher_node_hash_i)` commits to that item's fixed launcher-creation node. The DID
  spend is the single authorization; afterwards the DID is never needed again. Because each commitment
  coin's parent is the DID coin, and each launcher's parent is its commitment coin, **every NFT launcher
  id is deterministic and precomputed at commit time** and returned to the caller. Returns:
  `coin_spends` (the DID spend), `root` (a synthetic descriptor coin id binding the commit — the DID
  coin id), and `launcher_ids[]` (one per item, in order).
- **Claim (`build_lazy_mint_claim`)** — a NON-owner unrolls exactly ONE item, in a single bundle, with
  **no DID involvement**: (a) spend commitment coin `i` revealing its `P2Curried` node → it `create_early`s
  the launcher coin; (b) spend the launcher → eve NFT (`Launcher::mint_nft` with
  `transfer_condition: None`, recipient = the committed `claimer_puzzle_hash` for the direct/free mode,
  or the offer settlement puzzle for the paid mode); (c) the eve spend produces the child NFT owned by
  the recipient. The claimer's own coin funds the launcher's 1 mojo + any fee and asserts the
  commitment coin so the bundle is atomic. Returns the claim `coin_spends`.

### Creator attribution — provenance by lineage (not on-chain `current_owner`)

A trustless claim cannot set the NFT's on-chain `current_owner` to the creator DID: assigning a DID
owner emits a `TransferNft` whose `assignment_puzzle_announcement` **must be asserted by a DID spend**
(see `nft_launcher.rs`), i.e. the DID must be co-spent at claim — which breaks "anyone mints with no
further DID involvement." (secure-the-mint has the identical property: its eve mint leaves
`current_owner = None`.) So trustless lazy mint attributes the creator by **lineage**: every minted
NFT's launcher coin provably descends from the commitment coins created by the creator's single DID
spend (`launcher.parent == commit_coin`, `commit_coin.parent == did_coin`), and the royalty puzzle hash
is committed to the creator. The simulator test asserts exactly this chain. On-chain
`current_owner`-DID assignment per claim is possible only in a non-trustless "creator co-signs each
claim" mode and is **out of scope** — the hub keeps its honest deferral for that.

### Validated vs deferred (HONESTY — never claim a control is enforced when it isn't)

- **VALIDATED on the simulator** (`core/tests/lazy_mint_sim.rs`): fund a coin → create a DID →
  `build_lazy_mint_commit` for a 2–3 item collection and push it → `build_lazy_mint_claim` for one item
  **as a different party** and push it → the resulting NFT exists, is unspent, is owned by the claimer's
  puzzle hash, and its launcher lineage traces to the creator DID. This is real spend+validate, not a
  shape check.
- **Payment-gated claim** — the builder accepts a `LazyMintPolicy::PaymentGated { price, asset, payee }`
  and wires the recipient to the offer settlement puzzle (the offer-delegate path). The keyless builder
  emits the mint side; **atomic on-chain enforcement of the payment requires wrapping the eve mint in a
  full offer (settlement-payments puzzle + notarized payment) that the keyless single-bundle boundary
  does not assemble end-to-end** (taking an offer is a wallet settlement spend). It is therefore exposed
  but its enforcement is **DEFERRED**: the precise blocker is "offer settlement assembly + take is a
  wallet op outside the offline keyless builder." The hub keeps `LAZY_MINT_DEFERRED` honest for the paid
  flow until the offer-construction wave (DESIGN.md #35 "offer construction deferred") lands.
- **Allowlist (merkle) gating** — split honestly into what IS enforced and what is DEFERRED:
  - **ENFORCED — off-chain / builder-side (DONE, simulator-validated).** When a commit declares an
    `allowlist_root`, `build_lazy_mint_claim` **rejects** a claim with `Error::AllowlistDenied`
    (`ALLOWLIST_DENIED`) unless the caller supplies a `MerkleMembershipProof` that proves the claimer's
    **own `claimer_puzzle_hash`** is a member of that root. `verify_merkle_membership` recomputes the
    root from the leaf + proof using the EXACT `include/merkle_utils.clib` algorithm (leaf prefix
    `0x01`, node prefix `0x02`), byte-compatible with `chia_sdk_types::MerkleTree` — so the very proof a
    future on-chain claim puzzle would consume is the one validated here. The simulator test
    (`lazy_mint_sim.rs::allowlist_gated_commit_then_claim_with_proof`) proves a no-proof gated claim is
    rejected *before any spend* and a valid-proof claim still mints a live NFT on-chain. This stops the
    keyless builder from ever emitting a spend for a non-allowlisted address (the hub also gates
    off-chain). We prove the **claimer's own** puzzle hash (not the recipient/payee) so a paid drop
    cannot launder a non-allowlisted buyer through the payee.
  - **DEFERRED — trustless on-chain enforcement.** A claim spend assembled by *anything other than this
    builder* (a hand-rolled spend) is not bound to the off-chain check; truly trustless enforcement
    needs the merkle-verify to run **inside a compiled claim puzzle** that gates the `CreateCoin` to the
    proven address — a compiled `.clsp` that reintroduces exactly the custom-puzzle surface the
    SDK-primitive path avoids. The precise blocker: **"merkle membership must be enforced inside a
    compiled, audited claim puzzle; the puzzle is not yet authored/audited."** The reference shape is
    kept (`include/merkle_utils.clib` + the `lazy_mint_*_delegate.clsp` in `puzzles/`); the hub keeps
    `ALLOWLIST_ONCHAIN_DEFERRED` honest for the on-chain bit.

### Rust API (`core/src/lazy_mint.rs`)

```rust
pub enum LazyMintPolicy {
    /// Free / direct: claiming mints the NFT straight into `claimer_puzzle_hash` (no payment).
    DirectMint,
    /// Pay-to-mint: a claim must settle `price` of `asset` to `payee` (enforcement DEFERRED — see above).
    PaymentGated { price: u64, asset: PaymentAsset, payee: Bytes32 },
}

pub struct LazyMintItem { pub metadata: NftMediaMetadata, pub royalty_basis_points: u16 }

pub struct LazyMintCommitResponse {
    pub coin_spends: Vec<CoinSpend>,   // the single DID spend
    pub root: Bytes32,                 // the commit binding (the DID coin id)
    pub launcher_ids: Vec<Bytes32>,    // precomputed per-item NFT launcher ids, in order
    pub commit_coins: Vec<Coin>,       // the per-item commitment coins (amount 0) the claim spends
}

pub struct LazyMintClaimResponse {
    pub coin_spends: Vec<CoinSpend>,
    pub launcher_id: Bytes32,
    pub nft_coin: Coin,
}

/// The creator DID spends ONCE to precommit `items` into `collection`, attributed by lineage to `did`.
pub fn build_lazy_mint_commit(
    minter_synthetic_key: PublicKey, did: Did, collection: &Collection,
    items: &[LazyMintItem], policy: LazyMintPolicy, allowlist_root: Option<Bytes32>,
) -> Result<LazyMintCommitResponse, WalletError>;

/// A NON-owner unrolls + mints item `index` on demand, funding 1 mojo from `claimer_coins`. If the
/// commit declared an `allowlist_root`, `merkle_proof` is REQUIRED and is enforced here (off-chain):
/// it must prove `claimer_puzzle_hash` is a member, else `Err(WalletError::AllowlistDenied)`.
#[allow(clippy::too_many_arguments)]
pub fn build_lazy_mint_claim(
    claimer_synthetic_key: PublicKey, claimer_coins: Vec<Coin>, claimer_puzzle_hash: Bytes32,
    commit: &LazyMintTreeDescriptor, index: usize,
    merkle_proof: Option<MerkleMembershipProof>, fee: u64,
) -> Result<LazyMintClaimResponse, WalletError>;

/// The membership proof shape — mirrors `chia_sdk_types::MerkleProof` AND `include/merkle_utils.clib`.
pub struct MerkleMembershipProof {
    pub path: u32,           // LSB-first direction bits; bit i = direction at depth i
    pub proof: Vec<Bytes32>, // sibling hashes leaf→root
}

/// Recompute the merkle root implied by a leaf + proof (leaf = sha256(0x01||leaf); node =
/// sha256(0x02||left||right); path bit selects right(1)/left(0)). Byte-compatible with the SDK's
/// MerkleTree and the on-chain merkle_utils.clib, so a future claim puzzle computes the identical root.
pub fn merkle_membership_root(leaf: Bytes32, proof: &MerkleMembershipProof) -> Bytes32;

/// `merkle_membership_root(leaf, proof) == root`. The off-chain allowlist check (and what a hub/SDK
/// calls to gate a claim before building it).
pub fn verify_merkle_membership(leaf: Bytes32, proof: &MerkleMembershipProof, root: Bytes32) -> bool;
```

`LazyMintTreeDescriptor` is the keyless, serializable handle a caller persists after a commit (the
creator DID launcher id, the committed `collection`/`items`/`policy`/`allowlist_root`, and the
per-item `commit_coins` + `launcher_ids`) so a claimer — who never saw the commit call — can rebuild
the exact item `i` spend.

### Merkle proof shape (the allowlist contract)

The allowlist is a **merkle tree of the allowed claimer puzzle hashes** (32-byte leaves). The root is
the committed `allowlist_root`; a member's proof is `{ path, proof }` where `path` is the LSB-first
direction bits and `proof` is the sibling hashes leaf→root. The hashing is the standard Chia tree:
`sha256(0x01 || leaf)` for a leaf, `sha256(0x02 || left || right)` for an internal node, and at depth
`i` the `i`-th low bit of `path` says whether the running hash is the **right** (`1`) or **left** (`0`)
child. This is identical across three places, by construction and test: `chia_sdk_types::MerkleTree`
(how a producer builds the tree + proofs), `merkle_membership_root` (the Rust off-chain verifier), and
`include/merkle_utils.clib`'s `simplify_merkle_proof` (the reference clsp a future on-chain claim
puzzle would run). `core/tests/lazy_mint.rs::merkle_root_matches_chia_sdk_types` pins the Rust↔SDK
byte-equality. A hub/SDK builds the tree with `MerkleTree::new(leaves)`, ships each member their
`MerkleTree::proof(leaf)`, and gates with `verifyMerkleMembership` (or lets `buildLazyMintClaim`
reject a bad/absent proof).

### wasm exports (#40)
`buildLazyMintCommit(minterKey, did, collection, items, policy, allowlistRoot?) ->
{ coinSpends, root, launcherIds, commitCoins, descriptor }`,
`buildLazyMintClaim(claimerKey, claimerCoins, claimerPuzzleHash, descriptor, index, merkleProof?, fee)
-> { coinSpends, launcherId, nftCoin }` (throws `{ code: "ALLOWLIST_DENIED", message }` if the drop is
allowlist-gated and `merkleProof` is missing or doesn't prove `claimerPuzzleHash`), and
`verifyMerkleMembership(leaf, proof, root) -> boolean` (validate one proof without building a spend —
for off-chain gating in the hub/SDK). `policy` is `{ directMint: true }` or
`{ paymentGated: { price, asset, payee } }` (`asset` = `PaymentAsset`); `did` is the same `Did` shape
`mintNftWithDid`/`bulkMint` accept; `descriptor` is the `LazyMintTreeDescriptor` JSON the commit
returns; `merkleProof` is `{ path: number, proof: Uint8Array[] }`.

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
- **#46 monetization**: **dig-sdk** (`/spend` subpath) re-exports `buildPayment`, `buildCatPayment`,
  `paymentNonce`, `verifyPaymentReceipt`, `proveNftOwnership`, `proveCollectionMembership`,
  `readNftOwnership`; it should add an ergonomic `Monetization`/`Paywall` helper (issue nonce → build
  payment via the wallet → poll coinset for the owner coin → verify) so a dapp dev calls one method.
  **hub.dig.net** (`apps/web/lib/driver.js`) wires a "revenue" view + a paywall demo onto these. A
  dapp dev's flow: `paymentNonce` → `buildPayment` (sign via `window.chia`/Sage) → broadcast →
  `verifyPaymentReceipt` after confirm; NFT-gating: fetch the holder's latest NFT spend from
  `rpc.dig.net`/coinset → `proveNftOwnership`/`proveCollectionMembership`.

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
