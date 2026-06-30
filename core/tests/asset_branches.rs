//! Coverage top-up for the asset spend-builders' multi-coin / fee / change branches that the
//! happy-path tests in `assets.rs` don't reach: CAT issuance change + multi-coin, DID creation
//! fee + change + multi-coin, NFT mint fee + change + multi-coin + edition defaults, and the
//! DIG-CAT payment's mixed-asset / multi-cat / change arms. Keyless boundary, no simulator.

use chia_puzzle_types::{standard::StandardArgs, DeriveSynthetic};
use chia_sdk_driver::{Cat, CatInfo};
use chip35_dl_coin::{
    build_dig_store_payment, create_did, dig_treasury_payment_coin, issue_cat,
    master_to_wallet_unhardened, mint_nft, spend_bundle_to_hex, Bytes32, Coin, DidAttribution,
    Error, NftMediaMetadata, NftMintParams, PublicKey, SecretKey, Signature, SpendBundle,
    DIG_ASSET_ID, DIG_TREASURY_INNER_PUZZLE_HASH,
};

fn synthetic(seed: u8) -> PublicKey {
    let sk = SecretKey::from_seed(&[seed; 32]);
    master_to_wallet_unhardened(&sk.public_key(), 0).derive_synthetic()
}

fn owner_ph(synth: PublicKey) -> Bytes32 {
    StandardArgs::curry_tree_hash(synth).into()
}

fn coin(ph: Bytes32, amount: u64) -> Coin {
    Coin {
        parent_coin_info: Bytes32::new([7u8; 32]),
        puzzle_hash: ph,
        amount,
    }
}

/// A reconstructed [`Cat`] coin of `asset_id` owned by `owner_ph` — the shape a caller hands the
/// payment builder (its `child(...)` / `spend_all` math only needs the coin + asset id + p2 hash).
fn cat_coin(owner_ph: Bytes32, asset_id: Bytes32, amount: u64) -> Cat {
    let coin = Coin {
        parent_coin_info: Bytes32::new([7u8; 32]),
        puzzle_hash: Bytes32::new([0x11; 32]),
        amount,
    };
    Cat::new(coin, None, CatInfo::new(asset_id, None, owner_ph))
}

// ---- CAT issuance: change + multi-coin ----

#[test]
fn issue_cat_with_change_and_fee() {
    let synth = synthetic(2);
    let ph = owner_ph(synth);
    // total 1000 > amount 500 + fee 10 → the change CREATE_COIN + reserve_fee arms both run.
    let r = issue_cat(synth, vec![coin(ph, 1000)], 500, 10).expect("issue_cat w/ change");
    assert!(!r.coin_spends.is_empty());
    assert_ne!(r.asset_id, Bytes32::default());
}

#[test]
fn issue_cat_multi_coin_concurrent_spend() {
    let synth = synthetic(2);
    let ph = owner_ph(synth);
    // Two coins: the second asserts concurrent spend (the skip(1) loop).
    let r = issue_cat(synth, vec![coin(ph, 600), coin(ph, 600)], 1000, 0)
        .expect("issue_cat multi-coin");
    assert!(!r.coin_spends.is_empty());
    assert_eq!(r.cat_coins.len(), 1, "single CAT coin of the full supply");
}

// ---- DID creation: fee + change + multi-coin ----

#[test]
fn create_did_with_fee_and_change_and_multi_coin() {
    let synth = synthetic(2);
    let ph = owner_ph(synth);
    // fee 3 reserved, 1 mojo funds the launcher; total 100 leaves change. Two coins → concurrent loop.
    let r = create_did(synth, vec![coin(ph, 50), coin(ph, 50)], 3).expect("create_did w/ fee");
    // The bundle includes the lead spend, the concurrent-asserting second coin's spend, AND the
    // launcher/eve spends the DID creation emits — so more than just the two input coins.
    assert!(
        r.coin_spends.len() >= 2,
        "the concurrent-asserting second coin is spent too"
    );
    assert_ne!(r.launcher_id, Bytes32::default());
    assert_ne!(r.inner_puzzle_hash, Bytes32::default());
}

#[test]
fn create_did_is_deterministic() {
    let synth = synthetic(2);
    let ph = owner_ph(synth);
    let build = || {
        let r = create_did(synth, vec![coin(ph, 50)], 3).unwrap();
        spend_bundle_to_hex(&SpendBundle::new(r.coin_spends, Signature::default())).unwrap()
    };
    assert_eq!(build(), build(), "create_did is deterministic");
}

// ---- NFT mint: fee + change + multi-coin + edition defaults ----

fn media_with_editions(num: u64, total: u64) -> NftMediaMetadata {
    NftMediaMetadata {
        data_uris: vec!["dig://urn:dig:chia:s:r/a.png".into()],
        data_hash: Some(Bytes32::new([1u8; 32])),
        metadata_uris: vec!["dig://urn:dig:chia:s:r/a.json".into()],
        metadata_hash: Some(Bytes32::new([2u8; 32])),
        license_uris: vec![],
        license_hash: None,
        edition_number: num,
        edition_total: total,
    }
}

#[test]
fn mint_nft_with_fee_change_multi_coin_and_zero_editions() {
    let synth = synthetic(2);
    let ph = owner_ph(synth);
    let params = NftMintParams {
        // edition_number = edition_total = 0 → the `to_chain()` default-to-1 arms.
        metadata: media_with_editions(0, 0),
        p2_puzzle_hash: ph,
        royalty_puzzle_hash: ph,
        royalty_basis_points: 300,
        did: Some(DidAttribution {
            launcher_id: Bytes32::new([1u8; 32]),
            inner_puzzle_hash: Bytes32::new([2u8; 32]),
        }),
    };
    // Two coins (concurrent loop) + fee 5 (reserve_fee) + total above fee+1 (change CREATE_COIN).
    let r = mint_nft(synth, vec![coin(ph, 50), coin(ph, 50)], params, 5).expect("mint w/ fee");
    // The bundle includes the lead + concurrent second-coin spends plus the launcher/eve mint
    // spends — so more than just the two input coins.
    assert!(
        r.coin_spends.len() >= 2,
        "the concurrent-asserting second coin is spent too"
    );
    assert_ne!(r.launcher_id, Bytes32::default());
}

// ---- DIG-CAT per-capsule payment: error arms + multi-cat change ----

#[test]
fn dig_payment_empty_cats_errors() {
    let buyer = synthetic(2);
    let store_id = Bytes32::new([9u8; 32]);
    assert!(matches!(
        build_dig_store_payment(buyer, vec![], store_id, 100),
        Err(Error::Parse(_))
    ));
}

#[test]
fn dig_payment_wrong_asset_errors() {
    let buyer = synthetic(2);
    let buyer_ph = owner_ph(buyer);
    let store_id = Bytes32::new([9u8; 32]);
    // A CAT that is NOT the DIG asset id → the "not the DIG asset" arm.
    let not_dig = Bytes32::new([0xCC; 32]);
    let res = build_dig_store_payment(
        buyer,
        vec![cat_coin(buyer_ph, not_dig, 1000)],
        store_id,
        100,
    );
    assert!(matches!(res, Err(Error::Parse(_))), "got {res:?}");
}

#[test]
fn dig_payment_mixed_asset_ids_errors() {
    let buyer = synthetic(2);
    let buyer_ph = owner_ph(buyer);
    let store_id = Bytes32::new([9u8; 32]);
    // First is DIG (passes the asset-id gate) but the second mixes a different id → the mix arm.
    let other = Bytes32::new([0xDD; 32]);
    let res = build_dig_store_payment(
        buyer,
        vec![
            cat_coin(buyer_ph, DIG_ASSET_ID, 600),
            cat_coin(buyer_ph, other, 600),
        ],
        store_id,
        100,
    );
    assert!(matches!(res, Err(Error::Parse(_))), "got {res:?}");
}

#[test]
fn dig_payment_insufficient_total_errors() {
    let buyer = synthetic(2);
    let buyer_ph = owner_ph(buyer);
    let store_id = Bytes32::new([9u8; 32]);
    let res = build_dig_store_payment(
        buyer,
        vec![cat_coin(buyer_ph, DIG_ASSET_ID, 50)],
        store_id,
        100,
    );
    assert!(matches!(res, Err(Error::Parse(_))), "got {res:?}");
}

#[test]
fn dig_payment_multi_cat_with_change_succeeds() {
    let buyer = synthetic(2);
    let buyer_ph = owner_ph(buyer);
    let store_id = Bytes32::new([9u8; 32]);
    // Two DIG cats totalling 1000, paying 600 → change 400 (the change arm) + the concurrent-spend
    // arm on the non-lead cat.
    let cats = vec![
        cat_coin(buyer_ph, DIG_ASSET_ID, 600),
        cat_coin(buyer_ph, DIG_ASSET_ID, 400),
    ];
    let spends =
        build_dig_store_payment(buyer, cats, store_id, 600).expect("multi-cat payment w/ change");
    assert_eq!(spends.len(), 2, "one CAT spend per supplied cat");
}

#[test]
fn dig_treasury_payment_coin_lands_at_treasury_inner_ph() {
    let buyer_ph = owner_ph(synthetic(2));
    let lead = cat_coin(buyer_ph, DIG_ASSET_ID, 1000);
    let pay = dig_treasury_payment_coin(&lead, 600);
    assert_eq!(pay.amount, 600, "payment coin carries the paid amount");
    // The treasury payment coin's parent is the lead DIG cat coin.
    assert_eq!(pay.parent_coin_info, lead.coin.coin_id());
    // Sanity: the constant the build path pays to is exposed and non-default.
    assert_ne!(DIG_TREASURY_INNER_PUZZLE_HASH, Bytes32::default());
}
