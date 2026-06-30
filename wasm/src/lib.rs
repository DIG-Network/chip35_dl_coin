//! WebAssembly bindings for the isolated CHIP-0035 DataLayer store coin driver.
//!
//! Builds the coin spends to mint, update, and burn (melt) DataLayer stores.
//! Networking, signing, and key derivation are intentionally absent — the
//! caller signs the returned coin spends and assembles the spend bundle.

use wasm_bindgen::prelude::*;

mod asset_types;
mod lazy_mint_types;
mod monetization_types;
mod ts;
mod types;

/// Initialise the module. Call once at startup. Installs a panic hook (when the
/// `console-panic-hook` feature is on) so Rust panics surface in the JS console.
#[wasm_bindgen]
pub fn init() {
    #[cfg(feature = "console-panic-hook")]
    console_error_panic_hook::set_once();
}

/// The published npm package name (the scoped name the wasm publishes under). The Cargo crate name
/// is the unscoped `chip35-dl-coin-wasm`; `patch-pkg.mjs` rewrites the manifest to this scoped name
/// at publish time, so we pin it here for the runtime descriptor.
const PACKAGE_NAME: &str = "@dignetwork/chip35-dl-coin-wasm";

/// Every `#[wasm_bindgen]` builder/helper this module exports (the camelCase JS names), grouped by
/// family. Single source of truth for [`capabilities`] so the runtime descriptor cannot drift from
/// the actual surface — keep this in lockstep with the exports below.
const BUILDERS: &[&str] = &[
    // DataStore spend builders + serialization.
    "mintStore",
    "updateStoreMetadata",
    "updateStoreOwnership",
    "meltStore",
    "oracleSpend",
    "addFee",
    "digstoreOwnerHint",
    "dataStoreFromSpend",
    "spendBundleToHex",
    "hexSpendBundleToCoinSpends",
    // Per-capsule $DIG payment: mint is free of $DIG; a capsule (commit) pays the treasury.
    "buildDigStorePayment",
    "digTreasuryPaymentCoin",
    "digConstants",
    // DataStore delegation (hub Teams #43 + revocable deploy tokens #17).
    "adminDelegatedPuzzleFromKey",
    "writerDelegatedPuzzleFromKey",
    "oracleDelegatedPuzzle",
    // Asset toolkit (#33/#34/#35/#36/#38).
    "mintNft",
    "mintNftWithDid",
    "bulkMint",
    "generateItemMetadata",
    "createDid",
    "issueCat",
    "encodeOffer",
    "decodeOffer",
    "buildChip0007Metadata",
    "validateChip0007",
    "sha256",
    // In-dapp monetization (#46): payment, paywall, NFT-gating.
    "buildPayment",
    "buildCatPayment",
    "verifyPaymentReceipt",
    "paymentNonce",
    "proveNftOwnership",
    "proveCollectionMembership",
    "readNftOwnership",
    // Trustless lazy mint / mint-on-claim (#40): commit a collection once (DID), then anyone claims.
    "buildLazyMintCommit",
    "buildLazyMintClaim",
];

/// Every stable `UPPER_SNAKE` machine error code this module can surface — at a throwing export (the
/// thrown `{ code, message }` object) or as the `code` field of a `{ ok:false, code, error }` result.
/// Single source of truth for [`capabilities`] and the documented error catalogue. The `*_ERROR`/
/// argument codes come from the wasm boundary; the rest mirror the core enums
/// ([`chip35_dl_coin::Error`], [`chip35_dl_coin::GatingError`], [`chip35_dl_coin::PaywallError`]).
const ERROR_CODES: &[&str] = &[
    // Wasm-boundary codes.
    "INVALID_ARGUMENT",
    "SERDE_ERROR",
    // Core builder error (`chip35_dl_coin::Error`).
    "DRIVER_ERROR",
    "PARSE_ERROR",
    "PERMISSION_DENIED",
    // CHIP-0007 metadata validation/serialization (`chip35_dl_coin::MetadataError`).
    "METADATA_ERROR",
    // NFT-gating (`chip35_dl_coin::GatingError`).
    "NOT_AN_NFT",
    "WRONG_OWNER",
    "WRONG_COLLECTION",
    "WRONG_NFT",
    // Paywall (`chip35_dl_coin::PaywallError`).
    "WRONG_RECIPIENT",
    "INSUFFICIENT_AMOUNT",
    "WRONG_ASSET",
    "NONCE_MISMATCH",
];

/// The published package version (= the Cargo crate version = the npm package version). Lets a
/// consumer/agent feature-gate on exactly which build is loaded at runtime.
#[wasm_bindgen(js_name = "version")]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// A machine-readable descriptor of this module's surface, for runtime introspection by an agent or
/// consumer: `{ name, version, builders, errorCodes }`. `builders` is every exported builder/helper
/// (camelCase JS names); `errorCodes` is the catalogue of stable `UPPER_SNAKE` codes a caller may
/// branch on (thrown as `{ code, message }`, or carried as the `code` field of a `{ ok, code?, error? }`
/// result). One call yields the version + the full surface with zero out-of-band knowledge.
#[wasm_bindgen(js_name = "capabilities", unchecked_return_type = "Capabilities")]
pub fn capabilities() -> JsValue {
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Capabilities {
        name: &'static str,
        version: String,
        builders: &'static [&'static str],
        error_codes: &'static [&'static str],
    }
    // `to_js` cannot fail for this owned, plain struct; fall back to NULL defensively.
    to_js(&Capabilities {
        name: PACKAGE_NAME,
        version: env!("CARGO_PKG_VERSION").to_string(),
        builders: BUILDERS,
        error_codes: ERROR_CODES,
    })
    .unwrap_or(JsValue::NULL)
}

use crate::types::{
    bytes32, coin_spends_to_js, coins_from_js, delegated_puzzles_from_js, from_js, js_err,
    js_err_from, public_key, signature, to_js, DataStore, DelegatedPuzzle, SuccessResponse,
};
use chip35_dl_coin::{
    add_fee as core_add_fee, admin_delegated_puzzle_from_key as core_admin_dp,
    build_dig_store_payment as core_build_dig_store_payment,
    datastore_from_spend as core_datastore_from_spend,
    dig_treasury_payment_coin as core_dig_treasury_payment_coin,
    digstore_owner_hint as core_digstore_owner_hint,
    hex_spend_bundle_to_coin_spends as core_hex_to_css, melt_store as core_melt_store,
    mint_store as core_mint_store, oracle_delegated_puzzle as core_oracle_dp,
    oracle_spend as core_oracle_spend, spend_bundle_to_hex as core_sb_to_hex,
    update_store_metadata as core_update_meta, update_store_ownership as core_update_owner,
    writer_delegated_puzzle_from_key as core_writer_dp, DataStoreInnerSpend,
    SpendBundle as RustSpendBundle,
};

// ---------------------------------------------------------------------------
// Delegated-puzzle constructors (hub Teams #43 + revocable deploy tokens #17).
// A delegate is granted by adding the returned DelegatedPuzzle to a store's
// delegated-puzzle set (mintStore / updateStoreOwnership); revoked by replacing
// that set. The JS shape mirrors the DelegatedPuzzle the other builders accept,
// so the result drops straight into `delegatedPuzzles`/`newDelegatedPuzzles`.
// ---------------------------------------------------------------------------

/// Build the **Admin** delegated puzzle for a 48-byte synthetic public key (a hub Teams admin).
/// An admin may update the store AND change delegation (add/remove writers — i.e. revoke a deploy
/// token), but cannot transfer ownership. Returns a `DelegatedPuzzle` (`{ adminInnerPuzzleHash }`).
#[wasm_bindgen(
    js_name = "adminDelegatedPuzzleFromKey",
    unchecked_return_type = "DelegatedPuzzle"
)]
pub fn admin_delegated_puzzle_from_key(synthetic_key: &[u8]) -> Result<JsValue, JsValue> {
    let dp = core_admin_dp(&public_key(synthetic_key)?);
    to_js(&DelegatedPuzzle::from_native(&dp)?)
}

/// Build the **Writer** delegated puzzle for a 48-byte synthetic public key — a revocable deploy
/// token (#17) or a hub Teams writer (#43). A writer may advance the root (deploy a new capsule)
/// WITHOUT the owner seed, but may NOT change delegation or transfer ownership. Add it to a store
/// to issue the token; replace the store's delegated set to revoke it. Returns a `DelegatedPuzzle`
/// (`{ writerInnerPuzzleHash }`).
#[wasm_bindgen(
    js_name = "writerDelegatedPuzzleFromKey",
    unchecked_return_type = "DelegatedPuzzle"
)]
pub fn writer_delegated_puzzle_from_key(synthetic_key: &[u8]) -> Result<JsValue, JsValue> {
    let dp = core_writer_dp(&public_key(synthetic_key)?);
    to_js(&DelegatedPuzzle::from_native(&dp)?)
}

/// Build the **Oracle** delegated puzzle: anyone may spend the store for the fixed `oracle_fee`
/// (mojos) paid to the 32-byte `oracle_puzzle_hash`. Returns a `DelegatedPuzzle`
/// (`{ oraclePaymentPuzzleHash, oracleFee }`).
#[wasm_bindgen(
    js_name = "oracleDelegatedPuzzle",
    unchecked_return_type = "DelegatedPuzzle"
)]
pub fn oracle_delegated_puzzle(
    oracle_puzzle_hash: &[u8],
    oracle_fee: u64,
) -> Result<JsValue, JsValue> {
    let dp = core_oracle_dp(bytes32(oracle_puzzle_hash)?, oracle_fee);
    to_js(&DelegatedPuzzle::from_native(&dp)?)
}

/// Derive the digstore-scoped owner discovery hint for a 32-byte owner puzzle hash. The app
/// computes the SAME hint to enumerate the wallet's stores via coinset
/// get_coin_records_by_hint — it MUST match the hint mint_store emits. Returns 32 bytes.
#[wasm_bindgen(js_name = "digstoreOwnerHint")]
pub fn digstore_owner_hint(owner_puzzle_hash: &[u8]) -> Result<Vec<u8>, JsValue> {
    Ok(core_digstore_owner_hint(bytes32(owner_puzzle_hash)?).to_vec())
}

/// Reconstruct a DataStore from the coin spend that created its current coin (the launcher
/// spend for an eve store). Lets the app MELT a store it did not mint in-session: fetch the
/// creating spend from a full node, rebuild the DataStore here, then meltStore() it.
/// `coin_spend` is the wasm CoinSpend shape (Uint8Array fields); `prev_delegated_puzzles` is
/// the parent's delegated-puzzle list ([] for an owner-only store).
#[wasm_bindgen(js_name = "dataStoreFromSpend", unchecked_return_type = "DataStore")]
pub fn data_store_from_spend(
    #[wasm_bindgen(unchecked_param_type = "CoinSpend")] coin_spend: JsValue,
    #[wasm_bindgen(unchecked_param_type = "DelegatedPuzzle[]")] prev_delegated_puzzles: JsValue,
) -> Result<JsValue, JsValue> {
    let cs: crate::types::CoinSpend = from_js(coin_spend)?;
    let prev = delegated_puzzles_from_js(prev_delegated_puzzles)?;
    let ds = core_datastore_from_spend(cs.to_native()?, prev).map_err(js_err_from)?;
    to_js(&DataStore::from_native(&ds)?)
}

/// Build the spend bundle that launches a new DataLayer store singleton (see
/// [`chip35_dl_coin::mint_store`]). `program_hash` is an optional 32-byte size-proof; the rest of
/// the JS values mirror the core builder. Returns a `SuccessResponse` (`coinSpends` + `newStore`).
#[wasm_bindgen(js_name = "mintStore", unchecked_return_type = "SuccessResponse")]
#[allow(clippy::too_many_arguments)]
pub fn mint_store(
    minter_synthetic_key: &[u8],
    #[wasm_bindgen(unchecked_param_type = "Coin[]")] selected_coins: JsValue,
    root_hash: &[u8],
    label: Option<String>,
    description: Option<String>,
    bytes: Option<u64>,
    program_hash: Option<Vec<u8>>,
    owner_puzzle_hash: &[u8],
    #[wasm_bindgen(unchecked_param_type = "DelegatedPuzzle[]")] delegated_puzzles: JsValue,
    fee: u64,
) -> Result<JsValue, JsValue> {
    let program_hash = match program_hash {
        Some(sp) => Some(bytes32(&sp)?.to_string()),
        None => None,
    };
    let resp = core_mint_store(
        public_key(minter_synthetic_key)?,
        coins_from_js(selected_coins)?,
        bytes32(root_hash)?,
        label,
        description,
        bytes,
        program_hash,
        bytes32(owner_puzzle_hash)?,
        delegated_puzzles_from_js(delegated_puzzles)?,
        fee,
    )
    .map_err(js_err_from)?;
    to_js(&SuccessResponse::from_native(&resp)?)
}

/// Exercise a store's oracle delegated puzzle (see [`chip35_dl_coin::oracle_spend`]). The spender
/// pays the oracle fee plus `fee` from `selected_coins`. Returns a `SuccessResponse`.
#[wasm_bindgen(js_name = "oracleSpend", unchecked_return_type = "SuccessResponse")]
pub fn oracle_spend(
    spender_synthetic_key: &[u8],
    #[wasm_bindgen(unchecked_param_type = "Coin[]")] selected_coins: JsValue,
    #[wasm_bindgen(unchecked_param_type = "DataStore")] store: JsValue,
    fee: u64,
) -> Result<JsValue, JsValue> {
    let store: DataStore = from_js(store)?;
    let resp = core_oracle_spend(
        public_key(spender_synthetic_key)?,
        coins_from_js(selected_coins)?,
        store.to_native()?,
        fee,
    )
    .map_err(js_err_from)?;
    to_js(&SuccessResponse::from_native(&resp)?)
}

/// Burn (melt) a store singleton (see [`chip35_dl_coin::melt_store`]). Owner-authorized only.
/// Returns the melt `CoinSpend[]`.
#[wasm_bindgen(js_name = "meltStore", unchecked_return_type = "CoinSpend[]")]
pub fn melt_store(
    #[wasm_bindgen(unchecked_param_type = "DataStore")] store: JsValue,
    owner_public_key: &[u8],
) -> Result<JsValue, JsValue> {
    let store: DataStore = from_js(store)?;
    let css =
        core_melt_store(store.to_native()?, public_key(owner_public_key)?).map_err(js_err_from)?;
    coin_spends_to_js(&css)
}

/// Update a store's metadata (see [`chip35_dl_coin::update_store_metadata`]). Exactly one of
/// `owner_public_key`, `admin_public_key`, `writer_public_key` must be provided — it selects the
/// authorizing role. Returns a `SuccessResponse`.
#[wasm_bindgen(
    js_name = "updateStoreMetadata",
    unchecked_return_type = "SuccessResponse"
)]
#[allow(clippy::too_many_arguments)]
pub fn update_store_metadata(
    #[wasm_bindgen(unchecked_param_type = "DataStore")] store: JsValue,
    new_root_hash: &[u8],
    new_label: Option<String>,
    new_description: Option<String>,
    new_bytes: Option<u64>,
    new_program_hash: Option<Vec<u8>>,
    owner_public_key: Option<Vec<u8>>,
    admin_public_key: Option<Vec<u8>>,
    writer_public_key: Option<Vec<u8>>,
) -> Result<JsValue, JsValue> {
    let store: DataStore = from_js(store)?;
    let inner =
        match (&owner_public_key, &admin_public_key, &writer_public_key) {
            (Some(pk), None, None) => DataStoreInnerSpend::Owner(public_key(pk)?),
            (None, Some(pk), None) => DataStoreInnerSpend::Admin(public_key(pk)?),
            (None, None, Some(pk)) => DataStoreInnerSpend::Writer(public_key(pk)?),
            _ => return Err(js_err(
                "INVALID_ARGUMENT",
                "Exactly one of ownerPublicKey, adminPublicKey, writerPublicKey must be provided",
            )),
        };
    let new_program_hash = match new_program_hash {
        Some(sp) => Some(bytes32(&sp)?.to_string()),
        None => None,
    };
    let resp = core_update_meta(
        store.to_native()?,
        bytes32(new_root_hash)?,
        new_label,
        new_description,
        new_bytes,
        new_program_hash,
        inner,
    )
    .map_err(js_err_from)?;
    to_js(&SuccessResponse::from_native(&resp)?)
}

/// Transfer ownership and/or replace the delegated-puzzle set (see
/// [`chip35_dl_coin::update_store_ownership`]). Omitting `new_owner_puzzle_hash` keeps the
/// current owner. Exactly one of `owner_public_key`/`admin_public_key` must be provided. Returns
/// a `SuccessResponse`.
#[wasm_bindgen(
    js_name = "updateStoreOwnership",
    unchecked_return_type = "SuccessResponse"
)]
pub fn update_store_ownership(
    #[wasm_bindgen(unchecked_param_type = "DataStore")] store: JsValue,
    new_owner_puzzle_hash: Option<Vec<u8>>,
    #[wasm_bindgen(unchecked_param_type = "DelegatedPuzzle[]")] new_delegated_puzzles: JsValue,
    owner_public_key: Option<Vec<u8>>,
    admin_public_key: Option<Vec<u8>>,
) -> Result<JsValue, JsValue> {
    let store: DataStore = from_js(store)?;
    let native_store = store.to_native()?;
    let new_owner_ph = match new_owner_puzzle_hash {
        Some(ph) => bytes32(&ph)?,
        None => native_store.info.owner_puzzle_hash,
    };
    let inner = match (&owner_public_key, &admin_public_key) {
        (Some(pk), None) => DataStoreInnerSpend::Owner(public_key(pk)?),
        (None, Some(pk)) => DataStoreInnerSpend::Admin(public_key(pk)?),
        _ => {
            return Err(js_err(
                "INVALID_ARGUMENT",
                "Exactly one of ownerPublicKey, adminPublicKey must be provided",
            ))
        }
    };
    let resp = core_update_owner(
        native_store,
        new_owner_ph,
        delegated_puzzles_from_js(new_delegated_puzzles)?,
        inner,
    )
    .map_err(js_err_from)?;
    to_js(&SuccessResponse::from_native(&resp)?)
}

/// Serialize a spend bundle (`{coinSpends, aggregatedSignature}`) to its hex wire encoding
/// (see [`chip35_dl_coin::spend_bundle_to_hex`]).
#[wasm_bindgen(js_name = "spendBundleToHex")]
pub fn spend_bundle_to_hex(
    #[wasm_bindgen(unchecked_param_type = "SpendBundle")] spend_bundle: JsValue,
) -> Result<String, JsValue> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SbIn {
        coin_spends: Vec<crate::types::CoinSpend>,
        #[serde(with = "serde_bytes")]
        aggregated_signature: Vec<u8>,
    }
    let sb: SbIn = from_js(spend_bundle)?;
    let css = sb
        .coin_spends
        .iter()
        .map(crate::types::CoinSpend::to_native)
        .collect::<Result<Vec<_>, _>>()?;
    let bundle = RustSpendBundle::new(css, signature(&sb.aggregated_signature)?);
    core_sb_to_hex(&bundle).map_err(js_err_from)
}

/// Decode a hex-encoded spend bundle into its `CoinSpend[]` (see
/// [`chip35_dl_coin::hex_spend_bundle_to_coin_spends`]).
#[wasm_bindgen(
    js_name = "hexSpendBundleToCoinSpends",
    unchecked_return_type = "CoinSpend[]"
)]
pub fn hex_spend_bundle_to_coin_spends(hex: String) -> Result<JsValue, JsValue> {
    let css = core_hex_to_css(&hex).map_err(js_err_from)?;
    coin_spends_to_js(&css)
}

/// Build coin spends that reserve `fee` mojos from the spender's own coins while asserting
/// concurrent spend of `assert_coin_ids` (see [`chip35_dl_coin::add_fee`]). Lets a fee-less
/// singleton op (update/melt) carry a network fee. `selected_coins` is `Coin[]`,
/// `assert_coin_ids` is `Uint8Array[]` of 32-byte coin ids. Returns `CoinSpend[]`.
#[wasm_bindgen(js_name = "addFee", unchecked_return_type = "CoinSpend[]")]
pub fn add_fee(
    spender_synthetic_key: &[u8],
    #[wasm_bindgen(unchecked_param_type = "Coin[]")] selected_coins: JsValue,
    #[wasm_bindgen(unchecked_param_type = "Uint8Array[]")] assert_coin_ids: JsValue,
    fee: u64,
) -> Result<JsValue, JsValue> {
    let ids_raw: Vec<serde_bytes::ByteBuf> = from_js(assert_coin_ids)?;
    let ids = ids_raw
        .iter()
        .map(|b| bytes32(b))
        .collect::<Result<Vec<_>, _>>()?;
    let css = core_add_fee(
        public_key(spender_synthetic_key)?,
        coins_from_js(selected_coins)?,
        ids,
        fee,
    )
    .map_err(js_err_from)?;
    coin_spends_to_js(&css)
}

// ---------------------------------------------------------------------------
// Per-capsule $DIG payment (task #111). Minting a store is FREE of $DIG; a CAPSULE (commit /
// root-advance) pays the dynamic, USD-pegged per-capsule price in $DIG to the treasury. The commit
// path concatenates `buildDigStorePayment`'s CAT spends with `updateStoreMetadata`'s singleton spend
// into ONE bundle and signs them together (atomic). `mintStore` never carries a DIG payment.
// ---------------------------------------------------------------------------

/// Build the (UNSIGNED) DIG-CAT coin spends that pay `amount` base units of $DIG to the DIG treasury
/// for a capsule (commit) — the canonical per-capsule store payment (see
/// [`chip35_dl_coin::build_dig_store_payment`]). `dig_cats` is the buyer's `Cat[]` of the DIG asset
/// (as `chip0002_getAssetCoins` returns them); `store_id` is the capsule's launcher id (= store id);
/// `amount` is the dynamic, USD-pegged per-capsule price in DIG base units (an INPUT — never
/// hardcoded). The caller concatenates the returned `CoinSpend[]` with the commit's
/// `updateStoreMetadata` spends and signs the whole bundle. MINT does NOT call this (mint is free of
/// $DIG). Returns `CoinSpend[]`.
#[wasm_bindgen(
    js_name = "buildDigStorePayment",
    unchecked_return_type = "CoinSpend[]"
)]
pub fn build_dig_store_payment(
    buyer_synthetic_key: &[u8],
    #[wasm_bindgen(unchecked_param_type = "Cat[]")] dig_cats: JsValue,
    store_id: &[u8],
    amount: u64,
) -> Result<JsValue, JsValue> {
    let cats: Vec<crate::monetization_types::Cat> = from_js(dig_cats)?;
    let native_cats = cats
        .iter()
        .map(crate::monetization_types::Cat::to_native)
        .collect::<Result<Vec<_>, _>>()?;
    let css = core_build_dig_store_payment(
        public_key(buyer_synthetic_key)?,
        native_cats,
        bytes32(store_id)?,
        amount,
    )
    .map_err(js_err_from)?;
    coin_spends_to_js(&css)
}

/// The DIG-CAT payment coin a capsule (commit) pays to the treasury — what `buildDigStorePayment`
/// emits (see [`chip35_dl_coin::dig_treasury_payment_coin`]). `lead_dig_cat` is the lead DIG `Cat`
/// (its id is the new coin's parent); `amount` is the payment in DIG base units. Lets a caller pin /
/// verify the exact expected treasury coin without re-deriving the CAT wrap. Returns a `Coin`.
#[wasm_bindgen(js_name = "digTreasuryPaymentCoin", unchecked_return_type = "Coin")]
pub fn dig_treasury_payment_coin(
    #[wasm_bindgen(unchecked_param_type = "Cat")] lead_dig_cat: JsValue,
    amount: u64,
) -> Result<JsValue, JsValue> {
    let cat: crate::monetization_types::Cat = from_js(lead_dig_cat)?;
    let coin = core_dig_treasury_payment_coin(&cat.to_native()?, amount);
    to_js(&crate::types::Coin::from_native(&coin))
}

/// The cross-system $DIG-payment constants (mainnet): the DIG CAT `assetId` and the treasury INNER
/// `puzzleHash` every per-capsule payment settles to. Byte-identical across the ecosystem
/// (digstore-chain / hub). Lets an agent/consumer read them at runtime instead of hardcoding. Returns
/// `{ assetId: Uint8Array, treasuryInnerPuzzleHash: Uint8Array }`.
#[wasm_bindgen(js_name = "digConstants", unchecked_return_type = "DigConstants")]
pub fn dig_constants() -> JsValue {
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Out {
        #[serde(with = "serde_bytes")]
        asset_id: Vec<u8>,
        #[serde(with = "serde_bytes")]
        treasury_inner_puzzle_hash: Vec<u8>,
    }
    to_js(&Out {
        asset_id: chip35_dl_coin::DIG_ASSET_ID.to_vec(),
        treasury_inner_puzzle_hash: chip35_dl_coin::DIG_TREASURY_INNER_PUZZLE_HASH.to_vec(),
    })
    .unwrap_or(JsValue::NULL)
}

// ---------------------------------------------------------------------------
// Asset toolkit wasm exports (roadmap #33/#34/#35/#36).
// Thin wrappers: deserialize the JS shape, call the core builder, reserialize.
// ---------------------------------------------------------------------------

use crate::asset_types::{
    Chip0007Metadata as JsChip0007Metadata, Collection as JsCollection,
    ManifestItem as JsManifestItem, NftMintParams as JsNftMintParams,
};
use chip35_dl_coin::{
    build_bulk_mint as core_bulk_mint, create_did as core_create_did,
    decode_offer as core_decode_offer, encode_offer as core_encode_offer,
    generate_item_metadata as core_generate_item_metadata, issue_cat as core_issue_cat,
    mint_nft as core_mint_nft, mint_nft_with_did as core_mint_nft_with_did, sha256 as core_sha256,
    validate_uri_hash as core_validate_uri_hash, Did as RustDid, DidInfo as RustDidInfo,
};

/// SHA-256 of arbitrary bytes → 32-byte hash (the one true primitive for `data_hash`/
/// `metadata_hash`/`license_hash`). Returns the raw 32 bytes (`Uint8Array`).
#[wasm_bindgen(js_name = "sha256")]
pub fn sha256(bytes: &[u8]) -> Vec<u8> {
    core_sha256(bytes).to_vec()
}

/// Build a CHIP-0007 metadata document from a JS object and return its canonical JSON + the
/// `metadata_hash` (sha256 of that JSON). De-dupes the hand-computed badge metadata: callers stop
/// hand-rolling SHA-256. Returns `{ json: string, metadataHash: Uint8Array }`. Validates schema.
#[wasm_bindgen(
    js_name = "buildChip0007Metadata",
    unchecked_return_type = "Chip0007MetadataResult"
)]
pub fn build_chip0007_metadata(
    #[wasm_bindgen(unchecked_param_type = "Chip0007Metadata")] metadata: JsValue,
) -> Result<JsValue, JsValue> {
    let md: JsChip0007Metadata = from_js(metadata)?;
    let native = md.to_native();
    native
        .validate_schema()
        .map_err(|e| js_err("METADATA_ERROR", e.to_string()))?;
    let json = native
        .to_canonical_json()
        .map_err(|e| js_err("METADATA_ERROR", e.to_string()))?;
    let hash = core_sha256(json.as_bytes()).to_vec();

    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Out {
        json: String,
        #[serde(with = "serde_bytes")]
        metadata_hash: Vec<u8>,
    }
    to_js(&Out {
        json,
        metadata_hash: hash,
    })
}

/// Validate a CHIP-0007 document's schema, and (when the actual bytes are provided) that each
/// on-chain hash matches `sha256(bytes)` — the URI↔hash agreement check (#36). `assets` is
/// `{ dataBytes?, dataHash?, metadataBytes?, metadataHash?, licenseBytes?, licenseHash? }` (all
/// `Uint8Array`). Returns `{ ok: bool, errors: string[] }`.
#[wasm_bindgen(
    js_name = "validateChip0007",
    unchecked_return_type = "ValidationResult"
)]
pub fn validate_chip0007(
    #[wasm_bindgen(unchecked_param_type = "Chip0007Metadata")] metadata: JsValue,
    #[wasm_bindgen(unchecked_param_type = "Chip0007Assets")] assets: JsValue,
) -> Result<JsValue, JsValue> {
    #[derive(serde::Deserialize, Default)]
    #[serde(rename_all = "camelCase")]
    struct Assets {
        #[serde(default, with = "crate::types::serde_bytes_opt")]
        data_bytes: Option<Vec<u8>>,
        #[serde(default, with = "crate::types::serde_bytes_opt")]
        data_hash: Option<Vec<u8>>,
        #[serde(default, with = "crate::types::serde_bytes_opt")]
        metadata_bytes: Option<Vec<u8>>,
        #[serde(default, with = "crate::types::serde_bytes_opt")]
        metadata_hash: Option<Vec<u8>>,
        #[serde(default, with = "crate::types::serde_bytes_opt")]
        license_bytes: Option<Vec<u8>>,
        #[serde(default, with = "crate::types::serde_bytes_opt")]
        license_hash: Option<Vec<u8>>,
    }

    let md: JsChip0007Metadata = from_js(metadata)?;
    let native = md.to_native();
    let assets: Assets = if assets.is_undefined() || assets.is_null() {
        Assets::default()
    } else {
        from_js(assets)?
    };

    let mut errors: Vec<String> = Vec::new();
    if let Err(e) = native.validate_schema() {
        errors.push(e.to_string());
    }
    let mut check = |which: &'static str, bytes: &Option<Vec<u8>>, hash: &Option<Vec<u8>>| {
        if let (Some(b), Some(h)) = (bytes, hash) {
            match bytes32(h) {
                Ok(h32) => {
                    if let Err(e) = core_validate_uri_hash(which, b, h32) {
                        errors.push(e.to_string());
                    }
                }
                Err(_) => errors.push(format!("{which} hash is not 32 bytes")),
            }
        }
    };
    check("data", &assets.data_bytes, &assets.data_hash);
    check("metadata", &assets.metadata_bytes, &assets.metadata_hash);
    check("license", &assets.license_bytes, &assets.license_hash);

    #[derive(serde::Serialize)]
    struct Out {
        ok: bool,
        errors: Vec<String>,
    }
    to_js(&Out {
        ok: errors.is_empty(),
        errors,
    })
}

/// Mint a single NFT whose media lives in a DIG capsule (`dig://` URN + https gateway fallback URIs,
/// hashes computed from real bytes). `params` is `NftMintParams`. Returns
/// `{ coinSpends, launcherId, nftCoin }`.
#[wasm_bindgen(js_name = "mintNft", unchecked_return_type = "NftMintResult")]
pub fn mint_nft(
    minter_synthetic_key: &[u8],
    #[wasm_bindgen(unchecked_param_type = "Coin[]")] selected_coins: JsValue,
    #[wasm_bindgen(unchecked_param_type = "NftMintParams")] params: JsValue,
    fee: u64,
) -> Result<JsValue, JsValue> {
    let params: JsNftMintParams = from_js(params)?;
    let resp = core_mint_nft(
        public_key(minter_synthetic_key)?,
        coins_from_js(selected_coins)?,
        params.to_native()?,
        fee,
    )
    .map_err(js_err_from)?;

    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Out {
        coin_spends: Vec<crate::types::CoinSpend>,
        #[serde(with = "serde_bytes")]
        launcher_id: Vec<u8>,
        nft_coin: crate::types::Coin,
    }
    to_js(&Out {
        coin_spends: resp
            .coin_spends
            .iter()
            .map(crate::types::CoinSpend::from_native)
            .collect(),
        launcher_id: resp.launcher_id.to_vec(),
        nft_coin: crate::types::Coin::from_native(&resp.nft_coin),
    })
}

/// Mint a single NFT AUTHORIZED BY + attributed to a creator DID (#38). Unlike `mintNft` (which only
/// stamps the attribution), this composes the DID-acknowledgement spend into the bundle so the
/// on-chain owner assignment is genuinely authorized by the creator identity. `did` is the DID's
/// current on-chain coin + identifiers `{ didCoin, proof, launcherId, innerPuzzleHash }` (fetched
/// on-chain / from a prior `createDid`); `params` is `NftMintParams` (its `did` field is ignored —
/// the full `did` arg is the authority). `selected_coins` cover the `fee`. Returns
/// `{ coinSpends, launcherId, nftCoin }`.
#[wasm_bindgen(js_name = "mintNftWithDid", unchecked_return_type = "NftMintResult")]
pub fn mint_nft_with_did(
    minter_synthetic_key: &[u8],
    #[wasm_bindgen(unchecked_param_type = "Coin[]")] selected_coins: JsValue,
    #[wasm_bindgen(unchecked_param_type = "Did")] did: JsValue,
    #[wasm_bindgen(unchecked_param_type = "NftMintParams")] params: JsValue,
    fee: u64,
) -> Result<JsValue, JsValue> {
    let params: JsNftMintParams = from_js(params)?;
    let native_did = did_from_js(did)?;
    let resp = core_mint_nft_with_did(
        public_key(minter_synthetic_key)?,
        coins_from_js(selected_coins)?,
        native_did,
        params.to_native()?,
        fee,
    )
    .map_err(js_err_from)?;

    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Out {
        coin_spends: Vec<crate::types::CoinSpend>,
        #[serde(with = "serde_bytes")]
        launcher_id: Vec<u8>,
        nft_coin: crate::types::Coin,
    }
    to_js(&Out {
        coin_spends: resp
            .coin_spends
            .iter()
            .map(crate::types::CoinSpend::from_native)
            .collect(),
        launcher_id: resp.launcher_id.to_vec(),
        nft_coin: crate::types::Coin::from_native(&resp.nft_coin),
    })
}

/// Create a DID (creator identity) singleton. Returns
/// `{ coinSpends, launcherId, innerPuzzleHash, didCoin }`.
#[wasm_bindgen(js_name = "createDid", unchecked_return_type = "CreateDidResult")]
pub fn create_did(
    minter_synthetic_key: &[u8],
    #[wasm_bindgen(unchecked_param_type = "Coin[]")] selected_coins: JsValue,
    fee: u64,
) -> Result<JsValue, JsValue> {
    let resp = core_create_did(
        public_key(minter_synthetic_key)?,
        coins_from_js(selected_coins)?,
        fee,
    )
    .map_err(js_err_from)?;

    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Out {
        coin_spends: Vec<crate::types::CoinSpend>,
        #[serde(with = "serde_bytes")]
        launcher_id: Vec<u8>,
        #[serde(with = "serde_bytes")]
        inner_puzzle_hash: Vec<u8>,
        did_coin: crate::types::Coin,
    }
    to_js(&Out {
        coin_spends: resp
            .coin_spends
            .iter()
            .map(crate::types::CoinSpend::from_native)
            .collect(),
        launcher_id: resp.launcher_id.to_vec(),
        inner_puzzle_hash: resp.inner_puzzle_hash.to_vec(),
        did_coin: crate::types::Coin::from_native(&resp.did_coin),
    })
}

/// Issue a single-issuance (fixed-supply) CAT. Returns `{ coinSpends, assetId, catCoins }`.
#[wasm_bindgen(js_name = "issueCat", unchecked_return_type = "IssueCatResult")]
pub fn issue_cat(
    issuer_synthetic_key: &[u8],
    #[wasm_bindgen(unchecked_param_type = "Coin[]")] selected_coins: JsValue,
    amount: u64,
    fee: u64,
) -> Result<JsValue, JsValue> {
    let resp = core_issue_cat(
        public_key(issuer_synthetic_key)?,
        coins_from_js(selected_coins)?,
        amount,
        fee,
    )
    .map_err(js_err_from)?;

    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Out {
        coin_spends: Vec<crate::types::CoinSpend>,
        #[serde(with = "serde_bytes")]
        asset_id: Vec<u8>,
        cat_coins: Vec<crate::types::Coin>,
    }
    to_js(&Out {
        coin_spends: resp
            .coin_spends
            .iter()
            .map(crate::types::CoinSpend::from_native)
            .collect(),
        asset_id: resp.asset_id.to_vec(),
        cat_coins: resp
            .cat_coins
            .iter()
            .map(crate::types::Coin::from_native)
            .collect(),
    })
}

/// Generate per-item CHIP-0007 metadata documents for a collection from a parsed traits manifest
/// (#34). `collection` is `Collection`; `items` is `ManifestItem[]`. Returns
/// `Chip0007Metadata[]` (the off-chain JSON docs; the caller hashes + writes them into each capsule).
#[wasm_bindgen(
    js_name = "generateItemMetadata",
    unchecked_return_type = "Chip0007Metadata[]"
)]
pub fn generate_item_metadata(
    #[wasm_bindgen(unchecked_param_type = "Collection")] collection: JsValue,
    #[wasm_bindgen(unchecked_param_type = "ManifestItem[]")] items: JsValue,
) -> Result<JsValue, JsValue> {
    let col: JsCollection = from_js(collection)?;
    let items: Vec<JsManifestItem> = from_js(items)?;
    let native_col = col.to_native()?;
    let native_items = items
        .iter()
        .map(JsManifestItem::to_native)
        .collect::<Result<Vec<_>, _>>()?;
    let docs = core_generate_item_metadata(&native_col, &native_items);
    let out: Vec<JsChip0007Metadata> = docs.iter().map(JsChip0007Metadata::from_native).collect();
    to_js(&out)
}

/// Bulk-mint every item in a parsed traits manifest into a collection, attributed to a DID (#34).
/// `did` is the DID coin + identifiers `{ launcherId, innerPuzzleHash, didCoin }` (e.g. from a prior
/// `createDid`, fetched on-chain). Returns `{ coinSpends, launcherIds }`.
#[wasm_bindgen(js_name = "bulkMint", unchecked_return_type = "BulkMintResult")]
pub fn bulk_mint(
    minter_synthetic_key: &[u8],
    #[wasm_bindgen(unchecked_param_type = "Did")] did: JsValue,
    #[wasm_bindgen(unchecked_param_type = "Collection")] collection: JsValue,
    #[wasm_bindgen(unchecked_param_type = "ManifestItem[]")] items: JsValue,
    recipient_puzzle_hash: &[u8],
) -> Result<JsValue, JsValue> {
    let col: JsCollection = from_js(collection)?;
    let items: Vec<JsManifestItem> = from_js(items)?;
    let native_items = items
        .iter()
        .map(JsManifestItem::to_native)
        .collect::<Result<Vec<_>, _>>()?;
    let native_did = did_from_js(did)?;

    let resp = core_bulk_mint(
        public_key(minter_synthetic_key)?,
        native_did,
        &col.to_native()?,
        &native_items,
        bytes32(recipient_puzzle_hash)?,
    )
    .map_err(js_err_from)?;

    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Out {
        coin_spends: Vec<crate::types::CoinSpend>,
        launcher_ids: Vec<serde_bytes::ByteBuf>,
    }
    to_js(&Out {
        coin_spends: resp
            .coin_spends
            .iter()
            .map(crate::types::CoinSpend::from_native)
            .collect(),
        launcher_ids: resp
            .launcher_ids
            .iter()
            .map(|id| serde_bytes::ByteBuf::from(id.to_vec()))
            .collect(),
    })
}

/// Reconstruct a native `Did` from the JS shape
/// `{ didCoin, proof, launcherId, innerPuzzleHash, recoveryListHash?, numVerificationsRequired? }`.
///
/// The caller supplies the DID's CURRENT on-chain coin + lineage/eve proof (so `did.update` can
/// spend the real coin) plus its inner-puzzle hash (= the owner's standard puzzle hash). Simple DIDs
/// have `recoveryListHash = null` and `numVerificationsRequired = 1` and NIL metadata, matching
/// [`chip35_dl_coin::create_did`]. Metadata is treated as NIL (the simple-DID case the toolkit
/// creates); attributing through a DID carrying custom metadata is out of scope for v1.
fn did_from_js(value: JsValue) -> Result<RustDid, JsValue> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct JsDid {
        did_coin: crate::types::Coin,
        proof: crate::types::Proof,
        #[serde(with = "serde_bytes")]
        launcher_id: Vec<u8>,
        #[serde(with = "serde_bytes")]
        inner_puzzle_hash: Vec<u8>,
        #[serde(default, with = "crate::types::serde_bytes_opt")]
        recovery_list_hash: Option<Vec<u8>>,
        #[serde(default = "one")]
        num_verifications_required: u64,
    }
    fn one() -> u64 {
        1
    }
    let d: JsDid = from_js(value)?;
    let recovery = match &d.recovery_list_hash {
        Some(r) => Some(bytes32(r)?),
        None => None,
    };
    let info = RustDidInfo::new(
        bytes32(&d.launcher_id)?,
        recovery,
        d.num_verifications_required,
        chip35_dl_coin::HashedPtr::NIL,
        bytes32(&d.inner_puzzle_hash)?,
    );
    Ok(RustDid::new(
        d.did_coin.to_native()?,
        d.proof.to_native()?,
        info,
    ))
}

/// Encode a spend bundle (`{coinSpends, aggregatedSignature}`) into canonical offer text.
#[wasm_bindgen(js_name = "encodeOffer")]
pub fn encode_offer(
    #[wasm_bindgen(unchecked_param_type = "SpendBundle")] spend_bundle: JsValue,
) -> Result<String, JsValue> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SbIn {
        coin_spends: Vec<crate::types::CoinSpend>,
        #[serde(with = "serde_bytes")]
        aggregated_signature: Vec<u8>,
    }
    let sb: SbIn = from_js(spend_bundle)?;
    let css = sb
        .coin_spends
        .iter()
        .map(crate::types::CoinSpend::to_native)
        .collect::<Result<Vec<_>, _>>()?;
    let bundle = RustSpendBundle::new(css, signature(&sb.aggregated_signature)?);
    core_encode_offer(&bundle).map_err(js_err_from)
}

/// Decode canonical offer text into its spend bundle `{ coinSpends, aggregatedSignature }`.
#[wasm_bindgen(js_name = "decodeOffer", unchecked_return_type = "SpendBundle")]
pub fn decode_offer(text: String) -> Result<JsValue, JsValue> {
    let bundle = core_decode_offer(&text).map_err(js_err_from)?;
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Out {
        coin_spends: Vec<crate::types::CoinSpend>,
        #[serde(with = "serde_bytes")]
        aggregated_signature: Vec<u8>,
    }
    to_js(&Out {
        coin_spends: bundle
            .coin_spends
            .iter()
            .map(crate::types::CoinSpend::from_native)
            .collect(),
        aggregated_signature: bundle.aggregated_signature.to_bytes().to_vec(),
    })
}

// ---------------------------------------------------------------------------
// In-dapp monetization wasm exports (roadmap #46): payment, paywall (pay-to-unlock), NFT-gating,
// subscription scaffold. A dapp deployed on DIG EARNs — these are the inbound-economic primitives
// the dig-sdk + hub consume. Thin wrappers: deserialize the JS shape, call the core builder/helper,
// reserialize.
// ---------------------------------------------------------------------------

use crate::monetization_types::{
    opt_bytes32 as mon_opt_bytes32, Cat as JsCat, NftOwnershipProof as JsNftOwnershipProof,
    ObservedPayment as JsObservedPayment, PaymentAsset as JsPaymentAsset,
    PaymentReceipt as JsPaymentReceipt,
};
use chip35_dl_coin::{
    build_cat_payment as core_build_cat_payment, build_xch_payment as core_build_xch_payment,
    payment_nonce as core_payment_nonce, prove_collection_membership as core_prove_collection,
    prove_nft_ownership as core_prove_nft, read_nft_ownership as core_read_nft,
    verify_payment_receipt as core_verify_receipt,
};

/// SHA-256-derive a 32-byte unlock nonce from arbitrary request bytes (`dappId||resource||user`).
/// A dapp issues one nonce per unlock request, embeds it in the payment, and verifies it later. The
/// dapp may instead use any random 32 bytes — this is a deterministic convenience. Returns 32 bytes.
#[wasm_bindgen(js_name = "paymentNonce")]
pub fn payment_nonce(request_bytes: &[u8]) -> Vec<u8> {
    core_payment_nonce(request_bytes).to_vec()
}

/// Build the coin spends for a buyer to pay the dapp owner `amount` mojos of **XCH** (#46 payment).
/// `selected_coins` is the buyer's XCH `Coin[]`; `nonce` is the 32-byte unlock nonce. Returns
/// `{ coinSpends, receipt }` where `receipt` is the `PaymentReceipt` the paywall later verifies.
#[wasm_bindgen(js_name = "buildPayment", unchecked_return_type = "PaymentResponse")]
pub fn build_payment(
    buyer_synthetic_key: &[u8],
    #[wasm_bindgen(unchecked_param_type = "Coin[]")] selected_coins: JsValue,
    owner_puzzle_hash: &[u8],
    amount: u64,
    nonce: &[u8],
    fee: u64,
) -> Result<JsValue, JsValue> {
    let resp = core_build_xch_payment(
        public_key(buyer_synthetic_key)?,
        coins_from_js(selected_coins)?,
        bytes32(owner_puzzle_hash)?,
        amount,
        bytes32(nonce)?,
        fee,
    )
    .map_err(js_err_from)?;
    payment_response_to_js(&resp)
}

/// Build the coin spends for a buyer to pay the dapp owner `amount` base units of a **CAT** (incl.
/// DIG) (#46 payment). `selected_cats` is the buyer's `Cat[]` of ONE asset id (as
/// `chip0002_getAssetCoins` returns them); `nonce` is the 32-byte unlock nonce. The CAT ring nets to
/// zero, so carry any XCH network fee with a separate XCH coin via `addFee` asserting the lead CAT
/// coin id. Returns `{ coinSpends, receipt }`.
#[wasm_bindgen(js_name = "buildCatPayment", unchecked_return_type = "PaymentResponse")]
pub fn build_cat_payment(
    buyer_synthetic_key: &[u8],
    #[wasm_bindgen(unchecked_param_type = "Cat[]")] selected_cats: JsValue,
    owner_puzzle_hash: &[u8],
    amount: u64,
    nonce: &[u8],
) -> Result<JsValue, JsValue> {
    let cats: Vec<JsCat> = from_js(selected_cats)?;
    let native_cats = cats
        .iter()
        .map(JsCat::to_native)
        .collect::<Result<Vec<_>, _>>()?;
    let resp = core_build_cat_payment(
        public_key(buyer_synthetic_key)?,
        native_cats,
        bytes32(owner_puzzle_hash)?,
        amount,
        bytes32(nonce)?,
    )
    .map_err(js_err_from)?;
    payment_response_to_js(&resp)
}

/// Serialize a core `PaymentResponse` into the JS `{ coinSpends, receipt }` shape.
fn payment_response_to_js(resp: &chip35_dl_coin::PaymentResponse) -> Result<JsValue, JsValue> {
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Out {
        coin_spends: Vec<crate::types::CoinSpend>,
        receipt: JsPaymentReceipt,
    }
    to_js(&Out {
        coin_spends: resp
            .coin_spends
            .iter()
            .map(crate::types::CoinSpend::from_native)
            .collect(),
        receipt: JsPaymentReceipt::from_native(&resp.receipt),
    })
}

/// Verify an observed payment unlocks a paywall (#46 pay-to-unlock): it must pay `owner_puzzle_hash`,
/// in `required_asset`, at least `min_amount`, and (when `require_nonce` is a 32-byte value) carry
/// that nonce. `observed` is an `ObservedPayment` the dapp filled in after reading the owner's coin;
/// `required_asset` is a `PaymentAsset` (`{xch:true}` or `{assetId}`). Returns `{ ok, error? }` —
/// `ok:true` grants access, otherwise `error` is the human-readable denial reason.
#[wasm_bindgen(
    js_name = "verifyPaymentReceipt",
    unchecked_return_type = "PaywallResult"
)]
pub fn verify_payment_receipt(
    #[wasm_bindgen(unchecked_param_type = "ObservedPayment")] observed: JsValue,
    owner_puzzle_hash: &[u8],
    min_amount: u64,
    #[wasm_bindgen(unchecked_param_type = "PaymentAsset")] required_asset: JsValue,
    require_nonce: Option<Vec<u8>>,
) -> Result<JsValue, JsValue> {
    let observed: JsObservedPayment = from_js(observed)?;
    let asset: JsPaymentAsset = from_js(required_asset)?;
    let nonce = mon_opt_bytes32(require_nonce)?;
    let result = core_verify_receipt(
        &observed.to_native()?,
        bytes32(owner_puzzle_hash)?,
        min_amount,
        asset.to_native()?,
        nonce,
    );

    #[derive(serde::Serialize)]
    struct Out {
        ok: bool,
        /// Stable `UPPER_SNAKE` machine code on denial (a `PaywallError` code) — branch on this, not
        /// the prose `error`.
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<&'static str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    }
    to_js(&match result {
        Ok(()) => Out {
            ok: true,
            code: None,
            error: None,
        },
        Err(e) => Out {
            ok: false,
            code: Some(e.code()),
            error: Some(e.to_string()),
        },
    })
}

/// Prove an NFT (reconstructed from `parent_spend`, the coin spend that created its current coin) is
/// owned by `claimed_owner_puzzle_hash`, optionally gating on a specific NFT launcher id (#46 NFT-
/// gating). `parent_spend` is the wasm `CoinSpend` shape. Returns `{ ok, proof?, error? }` — on
/// success `proof` is the `NftOwnershipProof` (launcher id, owner, attributed DID, current coin id);
/// the caller still confirms `proof.nftCoinId` is unspent on-chain for liveness.
#[wasm_bindgen(js_name = "proveNftOwnership", unchecked_return_type = "GatingResult")]
pub fn prove_nft_ownership(
    #[wasm_bindgen(unchecked_param_type = "CoinSpend")] parent_spend: JsValue,
    claimed_owner_puzzle_hash: &[u8],
    required_nft: Option<Vec<u8>>,
) -> Result<JsValue, JsValue> {
    let cs: crate::types::CoinSpend = from_js(parent_spend)?;
    let required = mon_opt_bytes32(required_nft)?;
    let result = core_prove_nft(
        &cs.to_native()?,
        bytes32(claimed_owner_puzzle_hash)?,
        required,
    );
    gating_result_to_js(result)
}

/// Prove an NFT held by `claimed_owner_puzzle_hash` is a member of the collection/creator identified
/// by `required_did` (#46 collection-gating). Returns `{ ok, proof?, error? }`.
#[wasm_bindgen(
    js_name = "proveCollectionMembership",
    unchecked_return_type = "GatingResult"
)]
pub fn prove_collection_membership(
    #[wasm_bindgen(unchecked_param_type = "CoinSpend")] parent_spend: JsValue,
    claimed_owner_puzzle_hash: &[u8],
    required_did: &[u8],
) -> Result<JsValue, JsValue> {
    let cs: crate::types::CoinSpend = from_js(parent_spend)?;
    let result = core_prove_collection(
        &cs.to_native()?,
        bytes32(claimed_owner_puzzle_hash)?,
        bytes32(required_did)?,
    );
    gating_result_to_js(result)
}

/// Read an NFT's ownership facts (owner, attributed DID, launcher id, current coin id) from
/// `parent_spend` WITHOUT applying a gate — for dapps that want to decide in their own code.
/// Returns `{ ok, proof?, error? }`.
#[wasm_bindgen(js_name = "readNftOwnership", unchecked_return_type = "GatingResult")]
pub fn read_nft_ownership(
    #[wasm_bindgen(unchecked_param_type = "CoinSpend")] parent_spend: JsValue,
) -> Result<JsValue, JsValue> {
    let cs: crate::types::CoinSpend = from_js(parent_spend)?;
    gating_result_to_js(core_read_nft(&cs.to_native()?))
}

/// Serialize a gating `Result` into the JS `{ ok, proof?, error? }` shape.
fn gating_result_to_js(
    result: Result<chip35_dl_coin::NftOwnershipProof, chip35_dl_coin::GatingError>,
) -> Result<JsValue, JsValue> {
    #[derive(serde::Serialize)]
    struct Out {
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        proof: Option<JsNftOwnershipProof>,
        /// Stable `UPPER_SNAKE` machine code on failure (a `GatingError` code) — branch on this, not
        /// the prose `error`.
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<&'static str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    }
    to_js(&match result {
        Ok(p) => Out {
            ok: true,
            proof: Some(JsNftOwnershipProof::from_native(&p)),
            code: None,
            error: None,
        },
        Err(e) => Out {
            ok: false,
            proof: None,
            code: Some(e.code()),
            error: Some(e.to_string()),
        },
    })
}

// ---------------------------------------------------------------------------
// Trustless lazy mint / mint-on-claim wasm exports (roadmap #40). The creator DID precommits a whole
// collection ONCE (buildLazyMintCommit); afterwards ANYONE claims an individual NFT on demand with no
// further DID involvement (buildLazyMintClaim). Thin wrappers: deserialize the JS shape, call the core
// builder, reserialize. The `descriptor` a commit returns is an OPAQUE JSON string the caller persists
// and hands straight back to a claim.
// ---------------------------------------------------------------------------

use crate::lazy_mint_types::{
    descriptor_from_json, descriptor_to_json, LazyMintItem as JsLazyMintItem,
    LazyMintPolicy as JsLazyMintPolicy, MerkleMembershipProof as JsMerkleProof,
};
use chip35_dl_coin::{
    build_lazy_mint_claim as core_lazy_claim, build_lazy_mint_commit as core_lazy_commit,
};

/// The creator DID spends ONCE to precommit `items` into `collection`, attributed by lineage to `did`
/// (#40). `did` is the same `Did` shape `mintNftWithDid`/`bulkMint` accept (the DID's current on-chain
/// coin + identifiers). `policy` is `{ directMint: true }` (free) or
/// `{ paymentGated: { price, asset, payee } }` (payment ENFORCEMENT deferred — see the docs).
/// `allowlistRoot` is an optional 32-byte merkle root (on-chain enforcement deferred). Returns
/// `{ coinSpends, root, launcherIds, commitCoins, descriptor }` — `descriptor` is the opaque JSON
/// string a claimer passes back to `buildLazyMintClaim`.
#[wasm_bindgen(
    js_name = "buildLazyMintCommit",
    unchecked_return_type = "LazyMintCommitResult"
)]
pub fn build_lazy_mint_commit(
    minter_synthetic_key: &[u8],
    #[wasm_bindgen(unchecked_param_type = "Did")] did: JsValue,
    #[wasm_bindgen(unchecked_param_type = "Collection")] collection: JsValue,
    #[wasm_bindgen(unchecked_param_type = "LazyMintItem[]")] items: JsValue,
    #[wasm_bindgen(unchecked_param_type = "LazyMintPolicy")] policy: JsValue,
    allowlist_root: Option<Vec<u8>>,
) -> Result<JsValue, JsValue> {
    let col: JsCollection = from_js(collection)?;
    let items: Vec<JsLazyMintItem> = from_js(items)?;
    let native_items = items
        .iter()
        .map(JsLazyMintItem::to_native)
        .collect::<Result<Vec<_>, _>>()?;
    let policy: JsLazyMintPolicy = from_js(policy)?;
    let native_did = did_from_js(did)?;
    let allow_root = match allowlist_root {
        Some(r) => Some(bytes32(&r)?),
        None => None,
    };

    let resp = core_lazy_commit(
        public_key(minter_synthetic_key)?,
        native_did,
        &col.to_native()?,
        &native_items,
        policy.to_native()?,
        allow_root,
    )
    .map_err(js_err_from)?;

    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Out {
        coin_spends: Vec<crate::types::CoinSpend>,
        #[serde(with = "serde_bytes")]
        root: Vec<u8>,
        launcher_ids: Vec<serde_bytes::ByteBuf>,
        commit_coins: Vec<crate::types::Coin>,
        descriptor: String,
    }
    to_js(&Out {
        coin_spends: resp
            .coin_spends
            .iter()
            .map(crate::types::CoinSpend::from_native)
            .collect(),
        root: resp.root.to_vec(),
        launcher_ids: resp
            .launcher_ids
            .iter()
            .map(|id| serde_bytes::ByteBuf::from(id.to_vec()))
            .collect(),
        commit_coins: resp
            .commit_coins
            .iter()
            .map(crate::types::Coin::from_native)
            .collect(),
        descriptor: descriptor_to_json(&resp.descriptor())?,
    })
}

/// A NON-owner unrolls + mints item `index` of a precommitted collection on demand (#40), funding the
/// 1-mojo launcher (+ `fee`) from `claimerCoins`. `descriptor` is the opaque JSON string
/// `buildLazyMintCommit` returned; `claimerPuzzleHash` is the 32-byte recipient. `merkleProof` is
/// accepted for an allowlist-gated claim but on-chain enforcement is DEFERRED. Returns
/// `{ coinSpends, launcherId, nftCoin }`.
#[wasm_bindgen(
    js_name = "buildLazyMintClaim",
    unchecked_return_type = "LazyMintClaimResult"
)]
#[allow(clippy::too_many_arguments)]
pub fn build_lazy_mint_claim(
    claimer_synthetic_key: &[u8],
    #[wasm_bindgen(unchecked_param_type = "Coin[]")] claimer_coins: JsValue,
    claimer_puzzle_hash: &[u8],
    descriptor: String,
    index: usize,
    #[wasm_bindgen(unchecked_param_type = "MerkleMembershipProof")] merkle_proof: JsValue,
    fee: u64,
) -> Result<JsValue, JsValue> {
    let desc = descriptor_from_json(&descriptor)?;
    let proof = if merkle_proof.is_undefined() || merkle_proof.is_null() {
        None
    } else {
        let p: JsMerkleProof = from_js(merkle_proof)?;
        Some(p.to_native()?)
    };

    let resp = core_lazy_claim(
        public_key(claimer_synthetic_key)?,
        coins_from_js(claimer_coins)?,
        bytes32(claimer_puzzle_hash)?,
        &desc,
        index,
        proof,
        fee,
    )
    .map_err(js_err_from)?;

    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Out {
        coin_spends: Vec<crate::types::CoinSpend>,
        #[serde(with = "serde_bytes")]
        launcher_id: Vec<u8>,
        nft_coin: crate::types::Coin,
    }
    to_js(&Out {
        coin_spends: resp
            .coin_spends
            .iter()
            .map(crate::types::CoinSpend::from_native)
            .collect(),
        launcher_id: resp.launcher_id.to_vec(),
        nft_coin: crate::types::Coin::from_native(&resp.nft_coin),
    })
}
