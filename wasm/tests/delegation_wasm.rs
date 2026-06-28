//! `wasm-bindgen-test` coverage for the delegation builders (hub Teams #43 + deploy tokens #17).
//!
//! Run with: `wasm-pack test --node wasm` (from the repo root) — these execute the real
//! `#[wasm_bindgen]` constructors through the JS↔wasm boundary in a node wasm runtime. The richer
//! end-to-end issue→deploy→revoke flow lives in `tests/builders.mjs`; this is the in-crate
//! `wasm-bindgen-test` companion mandated by the TDD workflow.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

use chip35_dl_coin_wasm::{
    admin_delegated_puzzle_from_key, oracle_delegated_puzzle, writer_delegated_puzzle_from_key,
};

// A deterministic 48-byte synthetic key (the same fixture the node parity test uses).
const SYNTH_KEY_HEX: &str = "884b23d0b252b797ff8ea38095fd5fb0d41d6530707ddb3adb61a3b8be093cb778d82fc4c0d470701d051bccbddee75d";
// Its standard-puzzle tree hash (StandardArgs::curry_tree_hash).
const PUZZLE_HASH_HEX: &str = "349e7654559bcee11b6aa0aed2092ef9b241b25ecbf191232add4406473c420f";

fn key() -> Vec<u8> {
    hex::decode(SYNTH_KEY_HEX).unwrap()
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Dp {
    #[serde(default, with = "serde_bytes")]
    admin_inner_puzzle_hash: Option<Vec<u8>>,
    #[serde(default, with = "serde_bytes")]
    writer_inner_puzzle_hash: Option<Vec<u8>>,
    #[serde(default, with = "serde_bytes")]
    oracle_payment_puzzle_hash: Option<Vec<u8>>,
    oracle_fee: Option<u64>,
}

#[wasm_bindgen_test]
fn admin_dp_from_key_curries_standard_puzzle() {
    let dp: Dp =
        serde_wasm_bindgen::from_value(admin_delegated_puzzle_from_key(&key()).unwrap()).unwrap();
    assert_eq!(
        hex::encode(dp.admin_inner_puzzle_hash.unwrap()),
        PUZZLE_HASH_HEX
    );
    assert!(dp.writer_inner_puzzle_hash.is_none());
}

#[wasm_bindgen_test]
fn writer_dp_from_key_curries_standard_puzzle() {
    let dp: Dp =
        serde_wasm_bindgen::from_value(writer_delegated_puzzle_from_key(&key()).unwrap()).unwrap();
    assert_eq!(
        hex::encode(dp.writer_inner_puzzle_hash.unwrap()),
        PUZZLE_HASH_HEX
    );
    assert!(dp.admin_inner_puzzle_hash.is_none());
}

#[wasm_bindgen_test]
fn oracle_dp_carries_ph_and_fee() {
    let ph = hex::decode(PUZZLE_HASH_HEX).unwrap();
    let dp: Dp = serde_wasm_bindgen::from_value(oracle_delegated_puzzle(&ph, 7).unwrap()).unwrap();
    assert_eq!(
        hex::encode(dp.oracle_payment_puzzle_hash.unwrap()),
        PUZZLE_HASH_HEX
    );
    assert_eq!(dp.oracle_fee.unwrap(), 7);
}
