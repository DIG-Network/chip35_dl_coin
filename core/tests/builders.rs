use chip35_dl_coin::{
    hex_spend_bundle_to_coin_spends, master_to_wallet_unhardened, melt_store, mint_store,
    spend_bundle_to_hex, update_store_metadata, Bytes32, Coin, DataStoreInnerSpend, DelegatedPuzzle,
    SecretKey, Signature, SpendBundle,
};
use chia_puzzle_types::{standard::StandardArgs, DeriveSynthetic};

fn synthetic() -> chip35_dl_coin::PublicKey {
    let sk = SecretKey::from_seed(&[2u8; 32]);
    master_to_wallet_unhardened(&sk.public_key(), 0).derive_synthetic()
}

fn lead_coin(owner_ph: Bytes32) -> Coin {
    Coin {
        parent_coin_info: Bytes32::new([7u8; 32]),
        puzzle_hash: owner_ph,
        amount: 2,
    }
}

#[test]
fn mint_then_melt_then_update_and_roundtrip() {
    let synth = synthetic();
    let owner_ph: Bytes32 = StandardArgs::curry_tree_hash(synth).into();
    let admin = DelegatedPuzzle::Admin(StandardArgs::curry_tree_hash(synth));
    let root_hash = Bytes32::new([3u8; 32]);

    // MINT
    let mint = mint_store(
        synth,
        vec![lead_coin(owner_ph)],
        root_hash,
        Some("label".into()),
        Some("desc".into()),
        Some(42),
        None,
        owner_ph,
        vec![admin],
        0,
    )
    .expect("mint_store");
    assert!(!mint.coin_spends.is_empty(), "mint produced coin spends");
    assert_eq!(mint.new_datastore.info.metadata.root_hash, root_hash);

    // Determinism: minting the same inputs yields identical bytes.
    let mint2 = mint_store(
        synth,
        vec![lead_coin(owner_ph)],
        root_hash,
        Some("label".into()),
        Some("desc".into()),
        Some(42),
        None,
        owner_ph,
        vec![DelegatedPuzzle::Admin(StandardArgs::curry_tree_hash(synth))],
        0,
    )
    .expect("mint_store 2");
    let hex1 = spend_bundle_to_hex(&SpendBundle::new(mint.coin_spends.clone(), Signature::default()))
        .expect("hex1");
    let hex2 =
        spend_bundle_to_hex(&SpendBundle::new(mint2.coin_spends.clone(), Signature::default()))
            .expect("hex2");
    assert_eq!(hex1, hex2, "mint is deterministic");

    // Serialization round-trip.
    let back = hex_spend_bundle_to_coin_spends(&hex1).expect("decode");
    assert_eq!(back.len(), mint.coin_spends.len());

    // UPDATE METADATA (owner-authorized).
    let upd = update_store_metadata(
        mint.new_datastore.clone(),
        Bytes32::new([9u8; 32]),
        Some("l2".into()),
        None,
        None,
        None,
        DataStoreInnerSpend::Owner(synth),
    )
    .expect("update_store_metadata");
    assert!(!upd.coin_spends.is_empty());

    // BURN (melt).
    let melt = melt_store(mint.new_datastore.clone(), synth).expect("melt_store");
    assert_eq!(melt.len(), 1, "melt produces one coin spend");
}
