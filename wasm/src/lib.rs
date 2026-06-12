//! WebAssembly bindings for the isolated CHIP-0035 DataLayer store coin driver.
//!
//! Builds the coin spends to mint, update, and burn (melt) DataLayer stores.
//! Networking, signing, and key derivation are intentionally absent — the
//! caller signs the returned coin spends and assembles the spend bundle.

use wasm_bindgen::prelude::*;

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

#[wasm_bindgen(js_name = "meltStore")]
pub fn melt_store(store: JsValue, owner_public_key: &[u8]) -> Result<JsValue, JsValue> {
    let store: DataStore = from_js(store)?;
    let css = core_melt_store(store.to_native()?, public_key(owner_public_key)?)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    coin_spends_to_js(&css)
}

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
    let inner = match (&owner_public_key, &admin_public_key, &writer_public_key) {
        (Some(pk), None, None) => DataStoreInnerSpend::Owner(public_key(pk)?),
        (None, Some(pk), None) => DataStoreInnerSpend::Admin(public_key(pk)?),
        (None, None, Some(pk)) => DataStoreInnerSpend::Writer(public_key(pk)?),
        _ => {
            return Err(JsValue::from_str(
                "Exactly one of ownerPublicKey, adminPublicKey, writerPublicKey must be provided",
            ))
        }
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

#[wasm_bindgen(js_name = "hexSpendBundleToCoinSpends")]
pub fn hex_spend_bundle_to_coin_spends(hex: String) -> Result<JsValue, JsValue> {
    let css = core_hex_to_css(&hex).map_err(|e| JsValue::from_str(&e.to_string()))?;
    coin_spends_to_js(&css)
}

#[wasm_bindgen(js_name = "addFee")]
pub fn add_fee(
    spender_synthetic_key: &[u8],
    selected_coins: JsValue,    // Coin[]
    assert_coin_ids: JsValue,   // Uint8Array[] (32-byte coin ids)
    fee: u64,
) -> Result<JsValue, JsValue> {
    // CoinSpend[]
    let ids_raw: Vec<serde_bytes::ByteBuf> = from_js(assert_coin_ids)?;
    let ids = ids_raw.iter().map(|b| bytes32(b)).collect::<Result<Vec<_>, _>>()?;
    let css = core_add_fee(
        public_key(spender_synthetic_key)?,
        coins_from_js(selected_coins)?,
        ids,
        fee,
    )
    .map_err(|e| JsValue::from_str(&e.to_string()))?;
    coin_spends_to_js(&css)
}
