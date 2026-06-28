//! `wasm-bindgen-test` coverage for the in-dapp monetization wasm exports (roadmap #46): the paywall
//! check (`verifyPaymentReceipt`), the nonce derivation (`paymentNonce`), and the gating error path
//! (`readNftOwnership` on a non-NFT). These execute the real `#[wasm_bindgen]` functions across the
//! JS↔wasm boundary in a node wasm runtime.
//!
//! Run with: `wasm-pack test --node wasm` (from the repo root).
//!
//! The spend BUILDERS (`buildPayment` / `buildCatPayment` / `proveNftOwnership`) need a 48-byte
//! synthetic key + selected coins / a real NFT spend to exercise meaningfully; their construction
//! logic is covered by the native `tests/monetization.rs`. Here we cover the pure JS-boundary
//! helpers (verify/nonce) end-to-end and the gating error shape.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

use chip35_dl_coin_wasm::{payment_nonce, read_nft_ownership, verify_payment_receipt};

fn to_value<T: serde::Serialize>(v: &T) -> JsValue {
    serde_wasm_bindgen::to_value(v).unwrap()
}

#[wasm_bindgen_test]
fn payment_nonce_is_32_bytes() {
    let out = payment_nonce(b"dapp:article:user");
    assert_eq!(out.len(), 32);
}

#[wasm_bindgen_test]
fn verify_payment_receipt_grants_on_match() {
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Asset {
        xch: bool,
    }
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Observed {
        #[serde(with = "serde_bytes")]
        paid_to_puzzle_hash: Vec<u8>,
        amount: u64,
        asset: Asset,
        #[serde(with = "serde_bytes")]
        nonce: Vec<u8>,
    }
    let owner = vec![9u8; 32];
    let nonce = payment_nonce(b"unlock-1");
    let observed = Observed {
        paid_to_puzzle_hash: owner.clone(),
        amount: 250,
        asset: Asset { xch: true },
        nonce: nonce.clone(),
    };
    let res = verify_payment_receipt(
        to_value(&observed),
        &owner,
        250,
        to_value(&Asset { xch: true }),
        Some(nonce),
    )
    .expect("verify");

    #[derive(serde::Deserialize)]
    struct R {
        ok: bool,
    }
    let r: R = serde_wasm_bindgen::from_value(res).unwrap();
    assert!(r.ok);
}

#[wasm_bindgen_test]
fn verify_payment_receipt_denies_on_underpayment() {
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Asset {
        xch: bool,
    }
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Observed {
        #[serde(with = "serde_bytes")]
        paid_to_puzzle_hash: Vec<u8>,
        amount: u64,
        asset: Asset,
    }
    let owner = vec![9u8; 32];
    let observed = Observed {
        paid_to_puzzle_hash: owner.clone(),
        amount: 100,
        asset: Asset { xch: true },
    };
    let res = verify_payment_receipt(
        to_value(&observed),
        &owner,
        250,
        to_value(&Asset { xch: true }),
        None,
    )
    .expect("verify");

    #[derive(serde::Deserialize)]
    struct R {
        ok: bool,
        error: Option<String>,
    }
    let r: R = serde_wasm_bindgen::from_value(res).unwrap();
    assert!(!r.ok);
    assert!(r.error.is_some());
}

#[wasm_bindgen_test]
fn read_nft_ownership_flags_non_nft_spend() {
    // A coin spend with empty puzzle/solution is not an NFT — the gating result is { ok:false }.
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Coin {
        #[serde(with = "serde_bytes")]
        parent_coin_info: Vec<u8>,
        #[serde(with = "serde_bytes")]
        puzzle_hash: Vec<u8>,
        amount: u64,
    }
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct CoinSpend {
        coin: Coin,
        #[serde(with = "serde_bytes")]
        puzzle_reveal: Vec<u8>,
        #[serde(with = "serde_bytes")]
        solution: Vec<u8>,
    }
    let cs = CoinSpend {
        coin: Coin {
            parent_coin_info: vec![7u8; 32],
            puzzle_hash: vec![6u8; 32],
            amount: 1,
        },
        // `80` is the clvm serialization of nil (an empty program) — a valid program that is not an
        // NFT, so parsing succeeds but `parse_child` returns None → NotAnNft.
        puzzle_reveal: vec![0x80],
        solution: vec![0x80],
    };
    let res = read_nft_ownership(to_value(&cs)).expect("read");
    #[derive(serde::Deserialize)]
    struct R {
        ok: bool,
    }
    let r: R = serde_wasm_bindgen::from_value(res).unwrap();
    assert!(!r.ok);
}
