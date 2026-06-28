//! `wasm-bindgen-test` coverage for the asset-toolkit wasm exports.
//!
//! Run with: `wasm-pack test --node wasm` (from the repo root) — these execute the real
//! `#[wasm_bindgen]` functions through the JS↔wasm boundary in a node wasm runtime. The richer
//! end-to-end JS shape coverage lives in `tests/builders.mjs` (the repo's primary wasm harness);
//! this file is the in-crate `wasm-bindgen-test` companion mandated by the TDD workflow.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

// The exported functions under test.
use chip35_dl_coin_wasm::{build_chip0007_metadata, sha256 as wasm_sha256, validate_chip0007};

fn to_value<T: serde::Serialize>(v: &T) -> JsValue {
    serde_wasm_bindgen::to_value(v).unwrap()
}

#[wasm_bindgen_test]
fn sha256_returns_32_bytes() {
    let out = wasm_sha256(b"hello");
    assert_eq!(out.len(), 32);
}

#[wasm_bindgen_test]
fn build_chip0007_defaults_format_and_hashes() {
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct In {
        name: String,
    }
    let out = build_chip0007_metadata(to_value(&In {
        name: "DIG Punk #1".into(),
    }))
    .expect("build metadata");
    // The result is a JS object { json, metadataHash }; round-trip it back to confirm shape.
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Out {
        json: String,
        metadata_hash: serde_bytes::ByteBuf,
    }
    let out: Out = serde_wasm_bindgen::from_value(out).unwrap();
    assert!(out.json.contains("CHIP-0007"));
    assert_eq!(out.metadata_hash.len(), 32);
}

#[wasm_bindgen_test]
fn validate_chip0007_flags_hash_mismatch() {
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Md {
        name: String,
    }
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Assets {
        #[serde(with = "serde_bytes")]
        data_bytes: Vec<u8>,
        #[serde(with = "serde_bytes")]
        data_hash: Vec<u8>,
    }
    let res = validate_chip0007(
        to_value(&Md { name: "x".into() }),
        to_value(&Assets {
            data_bytes: b"real".to_vec(),
            data_hash: vec![0u8; 32], // wrong hash
        }),
    )
    .expect("validate");
    #[derive(serde::Deserialize)]
    struct R {
        ok: bool,
        errors: Vec<String>,
    }
    let r: R = serde_wasm_bindgen::from_value(res).unwrap();
    assert!(!r.ok);
    assert!(!r.errors.is_empty());
}
