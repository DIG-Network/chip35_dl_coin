//! `wasm-bindgen-test` coverage for the shared coin-selection + consolidation wasm exports
//! (epic #410 / #413): `selectCoins` (Ok / NeedsConsolidation / InsufficientFunds), and the
//! `buildCoinConsolidation` / `buildCatConsolidation` builders across the real JS↔wasm boundary in a
//! node wasm runtime.
//!
//! Run with: `wasm-pack test --node wasm` (from the repo root).
//!
//! The deep spend-construction logic (which coins merge, on-chain validity) is proven by the native
//! `core/tests/coin_select.rs` (incl. the Simulator gate); here we exercise the JS-boundary shapes.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

use chip35_dl_coin_wasm::{build_cat_consolidation, build_coin_consolidation, select_coins};

fn to_value<T: serde::Serialize>(v: &T) -> JsValue {
    serde_wasm_bindgen::to_value(v).unwrap()
}

// A valid 48-byte synthetic public key (the shared test fixture) whose standard puzzle hash the
// consolidation output self-sends to.
const SYNTH_KEY_HEX: &str = "884b23d0b252b797ff8ea38095fd5fb0d41d6530707ddb3adb61a3b8be093cb778d82fc4c0d470701d051bccbddee75d";

fn synth_key() -> Vec<u8> {
    let mut out = vec![0u8; SYNTH_KEY_HEX.len() / 2];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&SYNTH_KEY_HEX[i * 2..i * 2 + 2], 16).unwrap();
    }
    out
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Coin {
    #[serde(with = "serde_bytes")]
    parent_coin_info: Vec<u8>,
    #[serde(with = "serde_bytes")]
    puzzle_hash: Vec<u8>,
    amount: u64,
}

fn coin(parent: u8, amount: u64) -> Coin {
    Coin {
        parent_coin_info: vec![parent; 32],
        puzzle_hash: vec![6u8; 32],
        amount,
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CoinSpendOut {
    coin: Coin,
    #[serde(with = "serde_bytes")]
    #[allow(dead_code)]
    puzzle_reveal: Vec<u8>,
    #[serde(with = "serde_bytes")]
    #[allow(dead_code)]
    solution: Vec<u8>,
}

#[derive(serde::Serialize)]
struct XchAsset {
    xch: bool,
}

#[wasm_bindgen_test]
fn select_coins_ok_is_largest_first() {
    let coins = to_value(&vec![coin(1, 5), coin(2, 30), coin(3, 10), coin(4, 20)]);
    let res =
        select_coins(coins, 45, to_value(&XchAsset { xch: true }), None).expect("select_coins");

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct OkR {
        ok: bool,
        coins: Vec<Coin>,
        total: u64,
        change: u64,
        coin_count: u32,
    }
    let r: OkR = serde_wasm_bindgen::from_value(res).unwrap();
    assert!(r.ok);
    assert_eq!(r.coin_count, 2);
    assert_eq!(r.coins.len(), 2);
    assert_eq!(r.coins[0].amount, 30, "largest-first");
    assert_eq!(r.coins[1].amount, 20);
    assert_eq!(r.total, 50);
    assert_eq!(r.change, 5);
}

#[wasm_bindgen_test]
fn select_coins_signals_needs_consolidation() {
    let coins = to_value(&vec![coin(1, 10), coin(2, 10), coin(3, 10), coin(4, 10)]);
    let res =
        select_coins(coins, 40, to_value(&XchAsset { xch: true }), Some(3)).expect("select_coins");

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FailR {
        ok: bool,
        needs_consolidation: bool,
        available_coin_count: u32,
        available_total: u64,
        required: u64,
        cap: u32,
    }
    let r: FailR = serde_wasm_bindgen::from_value(res).unwrap();
    assert!(!r.ok);
    assert!(r.needs_consolidation);
    assert_eq!(r.available_coin_count, 4);
    assert_eq!(r.available_total, 40);
    assert_eq!(r.required, 40);
    assert_eq!(r.cap, 3);
}

#[wasm_bindgen_test]
fn select_coins_insufficient_is_distinct() {
    let coins = to_value(&vec![coin(1, 10), coin(2, 10)]);
    let res =
        select_coins(coins, 30, to_value(&XchAsset { xch: true }), None).expect("select_coins");

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FailR {
        ok: bool,
        needs_consolidation: bool,
        available_total: u64,
    }
    let r: FailR = serde_wasm_bindgen::from_value(res).unwrap();
    assert!(!r.ok);
    assert!(
        !r.needs_consolidation,
        "insufficient funds is not needsConsolidation"
    );
    assert_eq!(r.available_total, 20);
}

#[wasm_bindgen_test]
fn build_coin_consolidation_merges_smallest() {
    let coins = to_value(&vec![
        coin(1, 100),
        coin(2, 1),
        coin(3, 50),
        coin(4, 2),
        coin(5, 3),
    ]);
    let res = build_coin_consolidation(&synth_key(), coins, Some(3), 0).expect("consolidation");
    let spends: Vec<CoinSpendOut> = serde_wasm_bindgen::from_value(res).unwrap();
    assert_eq!(spends.len(), 3, "spends the 3 smallest coins");
    let mut amounts: Vec<u64> = spends.iter().map(|s| s.coin.amount).collect();
    amounts.sort_unstable();
    assert_eq!(
        amounts,
        vec![1, 2, 3],
        "only the smallest 3 coins are merged"
    );
}

#[wasm_bindgen_test]
fn build_coin_consolidation_requires_two() {
    let coins = to_value(&vec![coin(1, 10)]);
    assert!(build_coin_consolidation(&synth_key(), coins, Some(50), 0).is_err());
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CatInfoIn {
    #[serde(with = "serde_bytes")]
    asset_id: Vec<u8>,
    #[serde(with = "serde_bytes")]
    p2_puzzle_hash: Vec<u8>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CatIn {
    coin: Coin,
    info: CatInfoIn,
}

fn cat(parent: u8, amount: u64, asset: u8) -> CatIn {
    CatIn {
        coin: coin(parent, amount),
        info: CatInfoIn {
            asset_id: vec![asset; 32],
            p2_puzzle_hash: vec![6u8; 32],
        },
    }
}

#[wasm_bindgen_test]
fn build_cat_consolidation_merges_and_guards() {
    let cats = to_value(&vec![cat(1, 100, 0xAB), cat(2, 5, 0xAB), cat(3, 7, 0xAB)]);
    let res = build_cat_consolidation(&synth_key(), cats, Some(2)).expect("cat consolidation");
    let spends: Vec<CoinSpendOut> = serde_wasm_bindgen::from_value(res).unwrap();
    assert!(!spends.is_empty());

    // Requires >= 2 CATs.
    let one = to_value(&vec![cat(1, 10, 0xAB)]);
    assert!(build_cat_consolidation(&synth_key(), one, Some(50)).is_err());

    // Rejects mixed asset ids.
    let mixed = to_value(&vec![cat(1, 10, 0x01), cat(2, 10, 0x02)]);
    assert!(build_cat_consolidation(&synth_key(), mixed, Some(50)).is_err());
}
