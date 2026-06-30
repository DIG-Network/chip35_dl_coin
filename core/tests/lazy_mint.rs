//! Keyless determinism / structure tests for the trustless lazy mint (mint-on-claim) builders
//! (#40). These assert the builders produce well-formed, deterministic coin spends at the keyless
//! boundary (the `assets.rs` style — no simulator). The on-chain commit+claim is proven separately
//! in `lazy_mint_sim.rs` against the real Chia Simulator.

use chia_puzzle_types::{standard::StandardArgs, DeriveSynthetic};
use chia_sdk_driver::{Launcher, SpendContext, StandardLayer};
use chip35_dl_coin::{
    build_lazy_mint_claim, build_lazy_mint_commit, master_to_wallet_unhardened, sha256,
    spend_bundle_to_hex, Bytes32, Coin, Collection, Error, LazyMintItem, LazyMintPolicy,
    NftMediaMetadata, PublicKey, SecretKey, Signature, SpendBundle,
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

fn item(i: usize) -> LazyMintItem {
    LazyMintItem {
        metadata: NftMediaMetadata {
            data_uris: vec![format!("dig://urn:dig:chia:store:root/item{i}.png")],
            data_hash: Some(sha256(format!("bytes-{i}").as_bytes())),
            metadata_uris: vec![format!("dig://urn:dig:chia:store:root/item{i}.json")],
            metadata_hash: Some(sha256(format!("meta-{i}").as_bytes())),
            license_uris: vec![],
            license_hash: None,
            edition_number: 1,
            edition_total: 1,
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

/// Build a real DID owned by `synth` for use as the commit authority.
fn make_did(synth: PublicKey) -> chip35_dl_coin::Did {
    let ctx = &mut SpendContext::new();
    let p2 = StandardLayer::new(synth);
    let (_c, did) = Launcher::new(coin(owner_ph(synth), 1).coin_id(), 1)
        .create_simple_did(ctx, &p2)
        .expect("create did");
    did
}

// ---- commit ----

#[test]
fn commit_produces_spends_and_one_launcher_id_per_item() {
    let synth = synthetic(2);
    let did = make_did(synth);
    let col = test_collection(synth);
    let items = vec![item(0), item(1), item(2)];

    let r = build_lazy_mint_commit(synth, did, &col, &items, LazyMintPolicy::DirectMint, None)
        .expect("commit");

    assert!(!r.coin_spends.is_empty(), "commit emits the DID spend");
    assert_eq!(r.launcher_ids.len(), 3, "one launcher id per item");
    assert_eq!(r.commit_coins.len(), 3, "one commitment coin per item");
    // Every launcher id is distinct (per-item).
    assert_ne!(r.launcher_ids[0], r.launcher_ids[1]);
    assert_ne!(r.launcher_ids[1], r.launcher_ids[2]);
    // The commit binds to the DID coin id (the single authorization).
    assert_ne!(r.root, Bytes32::default());
}

#[test]
fn commit_is_deterministic() {
    let synth = synthetic(2);
    let col = test_collection(synth);
    let items = vec![item(0), item(1)];

    let build = || {
        let did = make_did(synth);
        let r = build_lazy_mint_commit(synth, did, &col, &items, LazyMintPolicy::DirectMint, None)
            .unwrap();
        (
            spend_bundle_to_hex(&SpendBundle::new(r.coin_spends, Signature::default())).unwrap(),
            r.launcher_ids,
        )
    };
    let (hex1, ids1) = build();
    let (hex2, ids2) = build();
    assert_eq!(hex1, hex2, "commit is deterministic for identical inputs");
    assert_eq!(ids1, ids2, "precomputed launcher ids are deterministic");
}

#[test]
fn commit_empty_items_errors() {
    let synth = synthetic(2);
    let did = make_did(synth);
    let col = test_collection(synth);
    assert!(matches!(
        build_lazy_mint_commit(synth, did, &col, &[], LazyMintPolicy::DirectMint, None),
        Err(Error::Parse(_))
    ));
}

// ---- claim (keyless shape; on-chain validity proven in lazy_mint_sim.rs) ----

#[test]
fn claim_produces_spends_for_one_item() {
    let minter = synthetic(2);
    let claimer = synthetic(9);
    let claimer_ph = owner_ph(claimer);
    let did = make_did(minter);
    let col = test_collection(minter);
    let items = vec![item(0), item(1)];

    let commit =
        build_lazy_mint_commit(minter, did, &col, &items, LazyMintPolicy::DirectMint, None)
            .expect("commit");
    let descriptor = commit.descriptor();

    let claim = build_lazy_mint_claim(
        claimer,
        vec![coin(claimer_ph, 5)],
        claimer_ph,
        &descriptor,
        1,
        None,
        0,
    )
    .expect("claim");

    assert!(!claim.coin_spends.is_empty());
    assert_eq!(
        claim.launcher_id, commit.launcher_ids[1],
        "claim mints exactly the precommitted launcher id for item 1"
    );
}

#[test]
fn claim_out_of_range_index_errors() {
    let minter = synthetic(2);
    let claimer = synthetic(9);
    let claimer_ph = owner_ph(claimer);
    let did = make_did(minter);
    let col = test_collection(minter);
    let items = vec![item(0)];

    let commit =
        build_lazy_mint_commit(minter, did, &col, &items, LazyMintPolicy::DirectMint, None)
            .expect("commit");
    let descriptor = commit.descriptor();

    assert!(matches!(
        build_lazy_mint_claim(
            claimer,
            vec![coin(claimer_ph, 5)],
            claimer_ph,
            &descriptor,
            5,
            None,
            0
        ),
        Err(Error::Parse(_))
    ));
}

#[test]
fn claim_no_coins_errors() {
    let minter = synthetic(2);
    let claimer = synthetic(9);
    let claimer_ph = owner_ph(claimer);
    let did = make_did(minter);
    let col = test_collection(minter);
    let items = vec![item(0)];

    let commit =
        build_lazy_mint_commit(minter, did, &col, &items, LazyMintPolicy::DirectMint, None)
            .expect("commit");
    let descriptor = commit.descriptor();

    assert!(matches!(
        build_lazy_mint_claim(claimer, vec![], claimer_ph, &descriptor, 0, None, 0),
        Err(Error::Parse(_))
    ));
}
