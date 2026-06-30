//! Coverage top-up for the PURE (no-chain) logic the happy-path suites skip: the
//! `PaymentAsset::asset_id` accessor, the human `Display` impls on `GatingError` / `PaywallError`
//! (only `code()` is pinned elsewhere), and the multi-coin / change branches of the XCH + CAT
//! payment builders.

use chia_puzzle_types::{standard::StandardArgs, DeriveSynthetic};
use chia_sdk_driver::{Cat, CatInfo};
use chip35_dl_coin::{
    build_cat_payment, build_xch_payment, master_to_wallet_unhardened, payment_nonce, Bytes32,
    Coin, GatingError, PaymentAsset, PaywallError, PublicKey, SecretKey,
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

fn cat_coin(owner_ph: Bytes32, asset_id: Bytes32, amount: u64) -> Cat {
    let coin = Coin {
        parent_coin_info: Bytes32::new([7u8; 32]),
        puzzle_hash: Bytes32::new([0x11; 32]),
        amount,
    };
    Cat::new(coin, None, CatInfo::new(asset_id, None, owner_ph))
}

// ---- PaymentAsset::asset_id accessor ----

#[test]
fn payment_asset_id_accessor() {
    assert_eq!(PaymentAsset::Xch.asset_id(), None, "XCH has no asset id");
    let id = Bytes32::new([0xAB; 32]);
    assert_eq!(
        PaymentAsset::Cat(id).asset_id(),
        Some(id),
        "a CAT returns its asset id"
    );
}

// ---- GatingError Display (human messages) ----

#[test]
fn gating_error_display_messages() {
    let a = Bytes32::new([1u8; 32]);
    let b = Bytes32::new([2u8; 32]);

    assert!(format!("{}", GatingError::NotAnNft).contains("not an NFT"));
    assert!(format!(
        "{}",
        GatingError::WrongOwner {
            expected: a,
            got: b
        }
    )
    .contains("owner"));
    assert!(format!(
        "{}",
        GatingError::WrongCollection {
            required: a,
            got: Some(b)
        }
    )
    .contains("collection"));
    assert!(format!(
        "{}",
        GatingError::WrongCollection {
            required: a,
            got: None
        }
    )
    .contains("collection"));
    assert!(format!(
        "{}",
        GatingError::WrongNft {
            required: a,
            got: b
        }
    )
    .contains("required"));
}

// ---- PaywallError Display (human messages) ----

#[test]
fn paywall_error_display_messages() {
    let a = Bytes32::new([1u8; 32]);
    let b = Bytes32::new([2u8; 32]);

    assert!(format!(
        "{}",
        PaywallError::WrongRecipient {
            expected: a,
            got: b
        }
    )
    .contains("recipient"));
    assert!(format!(
        "{}",
        PaywallError::InsufficientAmount {
            required: 10,
            got: 1
        }
    )
    .contains("below"));
    assert!(format!(
        "{}",
        PaywallError::WrongAsset {
            required: PaymentAsset::Xch,
            got: PaymentAsset::Cat(a)
        }
    )
    .contains("asset"));
    assert!(format!(
        "{}",
        PaywallError::NonceMismatch {
            expected: a,
            got: Some(b)
        }
    )
    .contains("nonce"));
    assert!(format!(
        "{}",
        PaywallError::NonceMismatch {
            expected: a,
            got: None
        }
    )
    .contains("nonce"));
}

// ---- XCH payment: multi-coin + change ----

#[test]
fn xch_payment_multi_coin_with_change() {
    let buyer = synthetic(2);
    let buyer_ph = owner_ph(buyer);
    let owner = owner_ph(synthetic(3));
    let nonce = payment_nonce(b"unlock:resource:user");

    // Two coins (concurrent-spend loop) + total above amount+fee (change CREATE_COIN).
    let r = build_xch_payment(
        buyer,
        vec![coin(buyer_ph, 500), coin(buyer_ph, 500)],
        owner,
        600,
        nonce,
        5,
    )
    .expect("xch payment multi-coin");
    assert_eq!(
        r.coin_spends.len(),
        2,
        "lead + concurrent-asserting second coin"
    );
    assert_eq!(r.receipt.amount, 600);
    assert_eq!(r.receipt.owner_puzzle_hash, owner);
    assert_eq!(r.receipt.asset, PaymentAsset::Xch);
    assert_eq!(r.receipt.nonce, nonce);
}

// ---- CAT payment: multi-coin + change ----

#[test]
fn cat_payment_multi_coin_with_change() {
    let buyer = synthetic(2);
    let buyer_ph = owner_ph(buyer);
    let owner = owner_ph(synthetic(3));
    let asset = Bytes32::new([0x42; 32]);
    let nonce = payment_nonce(b"unlock:cat");

    // Two cats totalling 1000, paying 600 → change 400 + the non-lead concurrent-spend arm.
    let r = build_cat_payment(
        buyer,
        vec![
            cat_coin(buyer_ph, asset, 600),
            cat_coin(buyer_ph, asset, 400),
        ],
        owner,
        600,
        nonce,
    )
    .expect("cat payment multi-coin");
    assert_eq!(r.coin_spends.len(), 2, "one CAT spend per supplied cat");
    assert_eq!(r.receipt.asset, PaymentAsset::Cat(asset));
    assert_eq!(r.receipt.amount, 600);
}
