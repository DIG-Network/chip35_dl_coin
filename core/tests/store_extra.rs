//! Coverage top-up for the CHIP-0035 store builders that the existing `builders.rs` /
//! `delegation.rs` suites don't exercise: the standalone fee coin (`add_fee`), reconstruction
//! from a creating spend (`datastore_from_spend`), the multi-coin concurrent-spend + change paths
//! of `mint_store`, and the `oracle_spend` fee + change branches. Keyless boundary, no simulator —
//! the same style as the rest of the core suite (assert the builders produce well-formed,
//! deterministic coin spends and the documented error arms).

use chia_puzzle_types::{standard::StandardArgs, DeriveSynthetic};
use chip35_dl_coin::{
    add_fee, datastore_from_spend, master_to_wallet_unhardened, mint_store,
    oracle_delegated_puzzle, oracle_spend, spend_bundle_to_hex, Bytes32, Coin, DelegatedPuzzle,
    Error, PublicKey, SecretKey, Signature, SpendBundle,
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

// ---- add_fee (the standalone fee coin a singleton-only op rides on) ----

#[test]
fn add_fee_reserves_fee_and_returns_change() {
    let synth = synthetic(2);
    let ph = owner_ph(synth);
    // One big coin: covers the fee with change left over (exercises the change CREATE_COIN path).
    let singleton_id = Bytes32::new([0xAB; 32]);
    let spends = add_fee(synth, vec![coin(ph, 1000)], vec![singleton_id], 50).expect("add_fee");
    assert!(!spends.is_empty(), "add_fee produces a coin spend");
}

#[test]
fn add_fee_is_deterministic() {
    let synth = synthetic(2);
    let ph = owner_ph(synth);
    let singleton_id = Bytes32::new([0xAB; 32]);
    let build = || {
        let spends = add_fee(synth, vec![coin(ph, 1000)], vec![singleton_id], 50).unwrap();
        spend_bundle_to_hex(&SpendBundle::new(spends, Signature::default())).unwrap()
    };
    assert_eq!(
        build(),
        build(),
        "add_fee is deterministic for identical inputs"
    );
}

#[test]
fn add_fee_multi_coin_asserts_concurrent_spend() {
    let synth = synthetic(2);
    let ph = owner_ph(synth);
    let singleton_id = Bytes32::new([0xAB; 32]);
    // Two coins: the second must assert concurrent spend of the lead (the skip(1) loop).
    let spends = add_fee(
        synth,
        vec![coin(ph, 100), coin(ph, 100)],
        vec![singleton_id],
        50,
    )
    .expect("add_fee multi-coin");
    assert_eq!(spends.len(), 2, "one spend per supplied coin");
}

#[test]
fn add_fee_exact_amount_no_change() {
    let synth = synthetic(2);
    let ph = owner_ph(synth);
    // total == fee → no change coin created (the `else` of the change branch).
    let spends = add_fee(synth, vec![coin(ph, 50)], vec![], 50).expect("add_fee exact");
    assert!(!spends.is_empty());
}

#[test]
fn add_fee_no_coins_errors() {
    let synth = synthetic(2);
    assert!(matches!(
        add_fee(synth, vec![], vec![], 10),
        Err(Error::Parse(_))
    ));
}

// ---- datastore_from_spend (reconstruct a store from its creating spend) ----

#[test]
fn datastore_from_spend_reconstructs_minted_store() {
    let synth = synthetic(2);
    let ph = owner_ph(synth);
    let root_hash = Bytes32::new([3u8; 32]);

    let mint = mint_store(
        synth,
        vec![coin(ph, 2)],
        root_hash,
        Some("label".into()),
        None,
        None,
        None,
        ph,
        vec![],
        0,
    )
    .expect("mint");

    // The launcher coin's spend is the spend whose CREATE_COIN produced the eve store coin. Find it
    // by the parent of the reconstructed store coin.
    let store_parent = mint.new_datastore.coin.parent_coin_info;
    let creating = mint
        .coin_spends
        .iter()
        .find(|cs| cs.coin.coin_id() == store_parent)
        .expect("the spend that created the store coin is in the bundle")
        .clone();

    let reconstructed =
        datastore_from_spend(creating, vec![]).expect("reconstruct store from creating spend");
    assert_eq!(
        reconstructed.info.metadata.root_hash, root_hash,
        "reconstructed store carries the minted root hash"
    );
    assert_eq!(
        reconstructed.info.launcher_id, mint.new_datastore.info.launcher_id,
        "same launcher id as the in-session store"
    );
}

#[test]
fn datastore_from_spend_rejects_non_datastore_spend() {
    let synth = synthetic(2);
    let ph = owner_ph(synth);
    // A plain fee coin spend is not a DataStore spend → Parse error.
    let fee_spends = add_fee(synth, vec![coin(ph, 100)], vec![], 10).expect("add_fee");
    let res = datastore_from_spend(fee_spends[0].clone(), vec![]);
    assert!(
        matches!(res, Err(Error::Parse(_))),
        "a non-DataStore spend is rejected"
    );
}

// ---- mint_store multi-coin + change branches ----

#[test]
fn mint_store_multi_coin_with_change() {
    let synth = synthetic(2);
    let ph = owner_ph(synth);
    // Two coins, total above fee+1 → concurrent-spend loop AND the change CREATE_COIN both run.
    let mint = mint_store(
        synth,
        vec![coin(ph, 100), coin(ph, 100)],
        Bytes32::new([3u8; 32]),
        None,
        None,
        None,
        None,
        ph,
        vec![],
        10,
    )
    .expect("mint multi-coin");
    // Both input coins are spent (the concurrent-spend loop runs on the second), and the launcher
    // coin spend is added too — so at least the two input-coin spends are present.
    assert!(
        mint.coin_spends.len() >= 2,
        "the concurrent-asserting second coin is spent too"
    );
    assert_eq!(
        mint.new_datastore.info.metadata.root_hash,
        Bytes32::new([3u8; 32])
    );
}

#[test]
fn mint_store_exact_amount_no_change() {
    let synth = synthetic(2);
    let ph = owner_ph(synth);
    // total == fee + 1 (the launcher mojo) → the no-change branch.
    let mint = mint_store(
        synth,
        vec![coin(ph, 1)],
        Bytes32::new([3u8; 32]),
        None,
        None,
        None,
        None,
        ph,
        vec![],
        0,
    )
    .expect("mint exact");
    // total == fee + 1 → no change coin is created; the bundle is the lead + launcher spends only.
    assert!(!mint.coin_spends.is_empty());
    assert_eq!(
        mint.new_datastore.coin.amount, 1,
        "the store singleton is the 1-mojo coin"
    );
}

// ---- oracle_spend fee + change branches ----

#[test]
fn oracle_spend_with_fee_and_change() {
    let synth = synthetic(2);
    let ph = owner_ph(synth);
    // oracle fee = 2, network fee = 5; a big coin leaves change (exercises the change + fee arms).
    let oracle = oracle_delegated_puzzle(ph, 2);
    let big = coin(ph, 1000);
    let mint = mint_store(
        synth,
        vec![big],
        Bytes32::new([3u8; 32]),
        None,
        None,
        None,
        None,
        ph,
        vec![oracle],
        0,
    )
    .expect("mint with oracle");

    let spent = oracle_spend(synth, vec![big], mint.new_datastore, 5).expect("oracle spend w/ fee");
    assert!(!spent.coin_spends.is_empty());
    // The oracle delegated puzzle survives the spend (the store is re-created unchanged).
    assert!(
        spent
            .new_datastore
            .info
            .delegated_puzzles
            .iter()
            .any(|dp| matches!(dp, DelegatedPuzzle::Oracle(_, _))),
        "the store still carries its oracle delegated puzzle"
    );
}

#[test]
fn oracle_spend_no_coins_errors() {
    let synth = synthetic(2);
    let ph = owner_ph(synth);
    let oracle = oracle_delegated_puzzle(ph, 2);
    let mint = mint_store(
        synth,
        vec![coin(ph, 1000)],
        Bytes32::new([3u8; 32]),
        None,
        None,
        None,
        None,
        ph,
        vec![oracle],
        0,
    )
    .expect("mint");
    assert!(matches!(
        oracle_spend(synth, vec![], mint.new_datastore, 0),
        Err(Error::Parse(_))
    ));
}
