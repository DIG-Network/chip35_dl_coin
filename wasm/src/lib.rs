//! WebAssembly bindings for the isolated CHIP-0035 DataLayer store coin driver.
//!
//! Builds the coin spends to mint, update, and burn (melt) DataLayer stores.
//! Networking, signing, and key derivation are intentionally absent — the
//! caller signs the returned coin spends and assembles the spend bundle.

use wasm_bindgen::prelude::*;

mod asset_types;
mod types;

/// Initialise the module. Call once at startup. Installs a panic hook (when the
/// `console-panic-hook` feature is on) so Rust panics surface in the JS console.
#[wasm_bindgen]
pub fn init() {
    #[cfg(feature = "console-panic-hook")]
    console_error_panic_hook::set_once();
}

use crate::types::{
    bytes32, coin_spends_to_js, coins_from_js, delegated_puzzles_from_js, from_js, public_key,
    signature, to_js, DataStore, SuccessResponse,
};
use chip35_dl_coin::{
    add_fee as core_add_fee, datastore_from_spend as core_datastore_from_spend,
    digstore_owner_hint as core_digstore_owner_hint,
    hex_spend_bundle_to_coin_spends as core_hex_to_css, melt_store as core_melt_store,
    mint_store as core_mint_store, oracle_spend as core_oracle_spend,
    spend_bundle_to_hex as core_sb_to_hex, update_store_metadata as core_update_meta,
    update_store_ownership as core_update_owner, DataStoreInnerSpend,
    SpendBundle as RustSpendBundle,
};

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
#[wasm_bindgen(js_name = "dataStoreFromSpend")]
pub fn data_store_from_spend(
    coin_spend: JsValue,
    prev_delegated_puzzles: JsValue,
) -> Result<JsValue, JsValue> {
    let cs: crate::types::CoinSpend = from_js(coin_spend)?;
    let prev = delegated_puzzles_from_js(prev_delegated_puzzles)?;
    let ds = core_datastore_from_spend(cs.to_native()?, prev)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    to_js(&DataStore::from_native(&ds)?)
}

/// Build the spend bundle that launches a new DataLayer store singleton (see
/// [`chip35_dl_coin::mint_store`]). `program_hash` is an optional 32-byte size-proof; the rest of
/// the JS values mirror the core builder. Returns a `SuccessResponse` (`coinSpends` + `newStore`).
#[wasm_bindgen(js_name = "mintStore")]
#[allow(clippy::too_many_arguments)]
pub fn mint_store(
    minter_synthetic_key: &[u8],
    selected_coins: JsValue,
    root_hash: &[u8],
    label: Option<String>,
    description: Option<String>,
    bytes: Option<u64>,
    program_hash: Option<Vec<u8>>,
    owner_puzzle_hash: &[u8],
    delegated_puzzles: JsValue,
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
    .map_err(|e| JsValue::from_str(&e.to_string()))?;
    to_js(&SuccessResponse::from_native(&resp)?)
}

/// Exercise a store's oracle delegated puzzle (see [`chip35_dl_coin::oracle_spend`]). The spender
/// pays the oracle fee plus `fee` from `selected_coins`. Returns a `SuccessResponse`.
#[wasm_bindgen(js_name = "oracleSpend")]
pub fn oracle_spend(
    spender_synthetic_key: &[u8],
    selected_coins: JsValue,
    store: JsValue,
    fee: u64,
) -> Result<JsValue, JsValue> {
    let store: DataStore = from_js(store)?;
    let resp = core_oracle_spend(
        public_key(spender_synthetic_key)?,
        coins_from_js(selected_coins)?,
        store.to_native()?,
        fee,
    )
    .map_err(|e| JsValue::from_str(&e.to_string()))?;
    to_js(&SuccessResponse::from_native(&resp)?)
}

/// Burn (melt) a store singleton (see [`chip35_dl_coin::melt_store`]). Owner-authorized only.
/// Returns the melt `CoinSpend[]`.
#[wasm_bindgen(js_name = "meltStore")]
pub fn melt_store(store: JsValue, owner_public_key: &[u8]) -> Result<JsValue, JsValue> {
    let store: DataStore = from_js(store)?;
    let css = core_melt_store(store.to_native()?, public_key(owner_public_key)?)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    coin_spends_to_js(&css)
}

/// Update a store's metadata (see [`chip35_dl_coin::update_store_metadata`]). Exactly one of
/// `owner_public_key`, `admin_public_key`, `writer_public_key` must be provided — it selects the
/// authorizing role. Returns a `SuccessResponse`.
#[wasm_bindgen(js_name = "updateStoreMetadata")]
#[allow(clippy::too_many_arguments)]
pub fn update_store_metadata(
    store: JsValue,
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
            _ => return Err(JsValue::from_str(
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
    .map_err(|e| JsValue::from_str(&e.to_string()))?;
    to_js(&SuccessResponse::from_native(&resp)?)
}

/// Transfer ownership and/or replace the delegated-puzzle set (see
/// [`chip35_dl_coin::update_store_ownership`]). Omitting `new_owner_puzzle_hash` keeps the
/// current owner. Exactly one of `owner_public_key`/`admin_public_key` must be provided. Returns
/// a `SuccessResponse`.
#[wasm_bindgen(js_name = "updateStoreOwnership")]
pub fn update_store_ownership(
    store: JsValue,
    new_owner_puzzle_hash: Option<Vec<u8>>,
    new_delegated_puzzles: JsValue,
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
            return Err(JsValue::from_str(
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
    .map_err(|e| JsValue::from_str(&e.to_string()))?;
    to_js(&SuccessResponse::from_native(&resp)?)
}

/// Serialize a spend bundle (`{coinSpends, aggregatedSignature}`) to its hex wire encoding
/// (see [`chip35_dl_coin::spend_bundle_to_hex`]).
#[wasm_bindgen(js_name = "spendBundleToHex")]
pub fn spend_bundle_to_hex(spend_bundle: JsValue) -> Result<String, JsValue> {
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
    core_sb_to_hex(&bundle).map_err(|e| JsValue::from_str(&e.to_string()))
}

/// Decode a hex-encoded spend bundle into its `CoinSpend[]` (see
/// [`chip35_dl_coin::hex_spend_bundle_to_coin_spends`]).
#[wasm_bindgen(js_name = "hexSpendBundleToCoinSpends")]
pub fn hex_spend_bundle_to_coin_spends(hex: String) -> Result<JsValue, JsValue> {
    let css = core_hex_to_css(&hex).map_err(|e| JsValue::from_str(&e.to_string()))?;
    coin_spends_to_js(&css)
}

/// Build coin spends that reserve `fee` mojos from the spender's own coins while asserting
/// concurrent spend of `assert_coin_ids` (see [`chip35_dl_coin::add_fee`]). Lets a fee-less
/// singleton op (update/melt) carry a network fee. `selected_coins` is `Coin[]`,
/// `assert_coin_ids` is `Uint8Array[]` of 32-byte coin ids. Returns `CoinSpend[]`.
#[wasm_bindgen(js_name = "addFee")]
pub fn add_fee(
    spender_synthetic_key: &[u8],
    selected_coins: JsValue,
    assert_coin_ids: JsValue,
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
    .map_err(|e| JsValue::from_str(&e.to_string()))?;
    coin_spends_to_js(&css)
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
    mint_nft as core_mint_nft, sha256 as core_sha256, validate_uri_hash as core_validate_uri_hash,
    Did as RustDid, DidInfo as RustDidInfo,
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
#[wasm_bindgen(js_name = "buildChip0007Metadata")]
pub fn build_chip0007_metadata(metadata: JsValue) -> Result<JsValue, JsValue> {
    let md: JsChip0007Metadata = from_js(metadata)?;
    let native = md.to_native();
    native
        .validate_schema()
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    let json = native
        .to_canonical_json()
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
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
#[wasm_bindgen(js_name = "validateChip0007")]
pub fn validate_chip0007(metadata: JsValue, assets: JsValue) -> Result<JsValue, JsValue> {
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
#[wasm_bindgen(js_name = "mintNft")]
pub fn mint_nft(
    minter_synthetic_key: &[u8],
    selected_coins: JsValue,
    params: JsValue,
    fee: u64,
) -> Result<JsValue, JsValue> {
    let params: JsNftMintParams = from_js(params)?;
    let resp = core_mint_nft(
        public_key(minter_synthetic_key)?,
        coins_from_js(selected_coins)?,
        params.to_native()?,
        fee,
    )
    .map_err(|e| JsValue::from_str(&e.to_string()))?;

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
#[wasm_bindgen(js_name = "createDid")]
pub fn create_did(
    minter_synthetic_key: &[u8],
    selected_coins: JsValue,
    fee: u64,
) -> Result<JsValue, JsValue> {
    let resp = core_create_did(
        public_key(minter_synthetic_key)?,
        coins_from_js(selected_coins)?,
        fee,
    )
    .map_err(|e| JsValue::from_str(&e.to_string()))?;

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
#[wasm_bindgen(js_name = "issueCat")]
pub fn issue_cat(
    issuer_synthetic_key: &[u8],
    selected_coins: JsValue,
    amount: u64,
    fee: u64,
) -> Result<JsValue, JsValue> {
    let resp = core_issue_cat(
        public_key(issuer_synthetic_key)?,
        coins_from_js(selected_coins)?,
        amount,
        fee,
    )
    .map_err(|e| JsValue::from_str(&e.to_string()))?;

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
#[wasm_bindgen(js_name = "generateItemMetadata")]
pub fn generate_item_metadata(collection: JsValue, items: JsValue) -> Result<JsValue, JsValue> {
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
#[wasm_bindgen(js_name = "bulkMint")]
pub fn bulk_mint(
    minter_synthetic_key: &[u8],
    did: JsValue,
    collection: JsValue,
    items: JsValue,
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
    .map_err(|e| JsValue::from_str(&e.to_string()))?;

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
pub fn encode_offer(spend_bundle: JsValue) -> Result<String, JsValue> {
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
    core_encode_offer(&bundle).map_err(|e| JsValue::from_str(&e.to_string()))
}

/// Decode canonical offer text into its spend bundle `{ coinSpends, aggregatedSignature }`.
#[wasm_bindgen(js_name = "decodeOffer")]
pub fn decode_offer(text: String) -> Result<JsValue, JsValue> {
    let bundle = core_decode_offer(&text).map_err(|e| JsValue::from_str(&e.to_string()))?;
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
