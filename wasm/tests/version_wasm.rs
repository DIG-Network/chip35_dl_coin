//! `wasm-bindgen-test` coverage for the runtime self-description surface (agent-friendly): the
//! `version()` + `capabilities()` exports an agent/consumer uses to introspect the package version
//! and the available builders at runtime, with zero out-of-band knowledge.
//!
//! Run with: `wasm-pack test --node wasm` (from the repo root).

#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

use chip35_dl_coin_wasm::{capabilities, version};

#[wasm_bindgen_test]
fn version_matches_cargo_pkg_version() {
    // The export must report the crate's own version (the npm package version), not a hard-coded
    // string — so a consumer can feature-gate on exactly what is loaded.
    assert_eq!(version(), env!("CARGO_PKG_VERSION"));
}

#[wasm_bindgen_test]
fn capabilities_descriptor_lists_the_surface() {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Caps {
        name: String,
        version: String,
        builders: Vec<String>,
        error_codes: Vec<String>,
    }
    let caps: Caps = serde_wasm_bindgen::from_value(capabilities()).unwrap();

    assert_eq!(caps.name, "@dignetwork/chip35-dl-coin-wasm");
    assert_eq!(caps.version, env!("CARGO_PKG_VERSION"));

    // A representative slice of each builder family must be advertised so an agent can discover the
    // surface from the descriptor alone.
    for expected in [
        "mintStore",
        "updateStoreMetadata",
        "meltStore",
        "adminDelegatedPuzzleFromKey",
        "mintNft",
        "issueCat",
        "buildPayment",
        "verifyPaymentReceipt",
        "proveNftOwnership",
    ] {
        assert!(
            caps.builders.iter().any(|b| b == expected),
            "capabilities().builders missing {expected}"
        );
    }

    // The catalogued machine error codes must be discoverable too.
    for expected in [
        "NOT_AN_NFT",
        "WRONG_OWNER",
        "INSUFFICIENT_AMOUNT",
        "PARSE_ERROR",
    ] {
        assert!(
            caps.error_codes.iter().any(|c| c == expected),
            "capabilities().errorCodes missing {expected}"
        );
    }
}
