//! Coverage top-up for the lazy-mint claim branches the existing `lazy_mint.rs` suite skips: the
//! fee + change + multi-coin arms of `build_lazy_mint_claim`, the insufficient-funds error, the
//! `PaymentGated` recipient policy, and the edition-default arms of an item's chain metadata.
//! Keyless boundary, no simulator (on-chain validity is proven in `lazy_mint_sim.rs`).

use chia_puzzle_types::{standard::StandardArgs, DeriveSynthetic};
use chia_sdk_driver::{Launcher, SpendContext, StandardLayer};
use chip35_dl_coin::{
    build_lazy_mint_claim, build_lazy_mint_commit, master_to_wallet_unhardened, sha256, Bytes32,
    Coin, Collection, Error, LazyMintItem, LazyMintPolicy, NftMediaMetadata, PaymentAsset,
    PublicKey, SecretKey,
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

/// An item whose editions are 0 → exercises the default-to-1 arms in `item_chain_metadata`.
fn item_zero_editions(i: usize) -> LazyMintItem {
    LazyMintItem {
        metadata: NftMediaMetadata {
            data_uris: vec![format!("dig://urn:dig:chia:store:root/item{i}.png")],
            data_hash: Some(sha256(format!("bytes-{i}").as_bytes())),
            metadata_uris: vec![format!("dig://urn:dig:chia:store:root/item{i}.json")],
            metadata_hash: Some(sha256(format!("meta-{i}").as_bytes())),
            license_uris: vec![],
            license_hash: None,
            edition_number: 0,
            edition_total: 0,
        },
        royalty_basis_points: 300,
    }
}

fn test_collection(synth: PublicKey) -> Collection {
    Collection {
        id: "lazy-col".into(),
        name: "DIG Lazy Punks".into(),
        attributes: vec![],
        royalty_puzzle_hash: owner_ph(synth),
        royalty_basis_points: 300,
    }
}

fn make_did(synth: PublicKey) -> chip35_dl_coin::Did {
    let ctx = &mut SpendContext::new();
    let p2 = StandardLayer::new(synth);
    let (_c, did) = Launcher::new(coin(owner_ph(synth), 1).coin_id(), 1)
        .create_simple_did(ctx, &p2)
        .expect("create did");
    did
}

/// A DirectMint claim with fee > 0, multiple claimer coins, and change left over exercises the
/// reserve-fee arm, the multi-coin concurrent-spend loop, and the change CREATE_COIN — plus the
/// zero-edition default-to-1 metadata arms via the item.
#[test]
fn claim_with_fee_change_and_multi_coin() {
    let minter = synthetic(2);
    let claimer = synthetic(9);
    let claimer_ph = owner_ph(claimer);
    let did = make_did(minter);
    let col = test_collection(minter);
    let items = vec![item_zero_editions(0), item_zero_editions(1)];

    let commit =
        build_lazy_mint_commit(minter, did, &col, &items, LazyMintPolicy::DirectMint, None)
            .expect("commit");
    let descriptor = commit.descriptor();

    // Two coins (concurrent loop) totalling 100, fee 10, reserved = fee + 1 = 11 → change 89.
    let claim = build_lazy_mint_claim(
        claimer,
        vec![coin(claimer_ph, 50), coin(claimer_ph, 50)],
        claimer_ph,
        &descriptor,
        1,
        None,
        10,
    )
    .expect("claim w/ fee + change + multi-coin");
    assert_eq!(claim.launcher_id, commit.launcher_ids[1]);
    assert!(
        claim.coin_spends.len() >= 2,
        "the second claimer coin is spent too"
    );
}

/// Claimer coins that cannot cover `fee + 1 mojo` are rejected before any spend is built.
#[test]
fn claim_insufficient_funds_errors() {
    let minter = synthetic(2);
    let claimer = synthetic(9);
    let claimer_ph = owner_ph(claimer);
    let did = make_did(minter);
    let col = test_collection(minter);
    let items = vec![item_zero_editions(0)];

    let commit =
        build_lazy_mint_commit(minter, did, &col, &items, LazyMintPolicy::DirectMint, None)
            .expect("commit");
    let descriptor = commit.descriptor();

    // total 5 < fee 10 + 1 launcher mojo.
    let res = build_lazy_mint_claim(
        claimer,
        vec![coin(claimer_ph, 5)],
        claimer_ph,
        &descriptor,
        0,
        None,
        10,
    );
    assert!(matches!(res, Err(Error::Parse(_))), "got {res:?}");
}

/// A `PaymentGated` policy still mints to the claimer (payment enforcement is deferred); this
/// exercises the `PaymentGated` arm of `claim_recipient`.
#[test]
fn claim_payment_gated_policy_mints_to_claimer() {
    let minter = synthetic(2);
    let claimer = synthetic(9);
    let claimer_ph = owner_ph(claimer);
    let payee = owner_ph(synthetic(3));
    let did = make_did(minter);
    let col = test_collection(minter);
    let items = vec![item_zero_editions(0)];

    let policy = LazyMintPolicy::PaymentGated {
        price: 1_000,
        asset: PaymentAsset::Xch,
        payee,
    };
    let commit =
        build_lazy_mint_commit(minter, did, &col, &items, policy, None).expect("commit gated");
    let descriptor = commit.descriptor();

    let claim = build_lazy_mint_claim(
        claimer,
        vec![coin(claimer_ph, 5)],
        claimer_ph,
        &descriptor,
        0,
        None,
        0,
    )
    .expect("payment-gated claim still mints (enforcement deferred)");
    assert_eq!(claim.launcher_id, commit.launcher_ids[0]);
}
