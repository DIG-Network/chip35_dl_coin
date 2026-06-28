//! Tests for the CHIP-0035 DataStore **delegation** builders (hub Teams #43 +
//! revocable deploy tokens #17).
//!
//! A DataStore singleton carries a list of [`DelegatedPuzzle`]s beside its owner:
//! - **Admin** = update the store + change delegation (add/remove admins/writers).
//! - **Writer** = create new generations (advance root = deploy) but NOT change delegation.
//! - **Oracle** = anyone may spend for a fixed fee.
//!
//! These tests mirror DataLayer-Driver's `admin_delegated_puzzle_from_key` /
//! `writer_delegated_puzzle_from_key` / `oracle_delegated_puzzle` shapes and exercise the
//! end-to-end mint → add-delegate → delegate-signed-root-advance flow that backs Teams and
//! deploy tokens.

use chia_puzzle_types::{standard::StandardArgs, DeriveSynthetic};
use chip35_dl_coin::{
    admin_delegated_puzzle_from_key, master_to_wallet_unhardened, mint_store,
    oracle_delegated_puzzle, update_store_metadata, update_store_ownership,
    writer_delegated_puzzle_from_key, Bytes32, Coin, DataStoreInnerSpend, DelegatedPuzzle, Error,
    SecretKey,
};

fn synthetic_from(seed: u8) -> chip35_dl_coin::PublicKey {
    let sk = SecretKey::from_seed(&[seed; 32]);
    master_to_wallet_unhardened(&sk.public_key(), 0).derive_synthetic()
}

fn lead_coin(owner_ph: Bytes32) -> Coin {
    Coin {
        parent_coin_info: Bytes32::new([7u8; 32]),
        puzzle_hash: owner_ph,
        amount: 2,
    }
}

/// The three constructors must produce EXACTLY DataLayer-Driver's shapes:
/// `Admin/Writer` curry the standard puzzle of the synthetic key; `Oracle` is the (ph, fee) pair.
#[test]
fn delegated_puzzle_constructors_match_datalayer_driver_shapes() {
    let key = synthetic_from(5);
    let expected_th = StandardArgs::curry_tree_hash(key);

    assert_eq!(
        admin_delegated_puzzle_from_key(&key),
        DelegatedPuzzle::Admin(expected_th),
        "admin DP = Admin(curry_tree_hash(synthetic_key))"
    );
    assert_eq!(
        writer_delegated_puzzle_from_key(&key),
        DelegatedPuzzle::Writer(expected_th),
        "writer DP = Writer(curry_tree_hash(synthetic_key))"
    );

    let oracle_ph = Bytes32::new([8u8; 32]);
    assert_eq!(
        oracle_delegated_puzzle(oracle_ph, 1000),
        DelegatedPuzzle::Oracle(oracle_ph, 1000),
        "oracle DP = Oracle(ph, fee)"
    );
}

/// Admin and writer DPs derived from the same key share the curried tree hash (it is keyed only by
/// the synthetic key, not the role), so a key authorizes whichever role the owner granted it.
#[test]
fn admin_and_writer_from_same_key_share_tree_hash() {
    let key = synthetic_from(3);
    let (DelegatedPuzzle::Admin(a), DelegatedPuzzle::Writer(w)) = (
        admin_delegated_puzzle_from_key(&key),
        writer_delegated_puzzle_from_key(&key),
    ) else {
        panic!("expected Admin and Writer variants");
    };
    assert_eq!(
        a, w,
        "same key → same standard-puzzle tree hash for both roles"
    );
}

/// Teams (#43): an OWNER mints a store, then ADDS a teammate as a writer by replacing the
/// delegated-puzzle set via `update_store_ownership` (owner-authorized). The new store carries the
/// writer DP.
#[test]
fn owner_adds_writer_delegate_teams_add_member() {
    let owner = synthetic_from(2);
    let teammate = synthetic_from(4);
    let owner_ph: Bytes32 = StandardArgs::curry_tree_hash(owner).into();

    // Mint owner-only (no delegates yet).
    let mint = mint_store(
        owner,
        vec![lead_coin(owner_ph)],
        Bytes32::new([3u8; 32]),
        None,
        None,
        None,
        None,
        owner_ph,
        vec![],
        0,
    )
    .expect("mint");
    assert!(mint.new_datastore.info.delegated_puzzles.is_empty());

    // Owner adds the teammate as a writer (the "add member" op).
    let writer = writer_delegated_puzzle_from_key(&teammate);
    let added = update_store_ownership(
        mint.new_datastore,
        owner_ph,
        vec![writer],
        DataStoreInnerSpend::Owner(owner),
    )
    .expect("owner adds writer delegate");
    assert_eq!(
        added.new_datastore.info.delegated_puzzles,
        vec![writer],
        "store now carries the teammate's writer delegate"
    );
}

/// Deploy token (#17): a store minted with a WRITER delegate lets that writer key advance the root
/// (= deploy a new capsule) WITHOUT the owner seed. This is the deploy-token / team-member commit.
#[test]
fn writer_delegate_advances_root_without_owner_seed() {
    let owner = synthetic_from(2);
    let deploy_key = synthetic_from(6); // the CI deploy key — NOT the owner seed
    let owner_ph: Bytes32 = StandardArgs::curry_tree_hash(owner).into();

    // Mint with the deploy key pre-authorized as a writer.
    let writer = writer_delegated_puzzle_from_key(&deploy_key);
    let mint = mint_store(
        owner,
        vec![lead_coin(owner_ph)],
        Bytes32::new([3u8; 32]),
        None,
        None,
        None,
        None,
        owner_ph,
        vec![writer],
        0,
    )
    .expect("mint with writer delegate");

    // The deploy key (writer) advances the root — signed by the writer, not the owner.
    let advanced = update_store_metadata(
        mint.new_datastore,
        Bytes32::new([9u8; 32]), // new root = a new capsule
        None,
        None,
        None,
        None,
        DataStoreInnerSpend::Writer(deploy_key),
    )
    .expect("writer advances root");
    assert_eq!(
        advanced.new_datastore.info.metadata.root_hash,
        Bytes32::new([9u8; 32]),
        "writer advanced the store to the new root"
    );
}

/// Admin delegate (#43 team admin): can advance the root AND change the delegated-puzzle set
/// (revoke a writer = revoke a deploy token), but cannot transfer OWNERSHIP outright.
#[test]
fn admin_delegate_changes_delegation_revoke_deploy_token() {
    let owner = synthetic_from(2);
    let admin_key = synthetic_from(7);
    let revoked_writer_key = synthetic_from(8);
    let owner_ph: Bytes32 = StandardArgs::curry_tree_hash(owner).into();

    // Mint with an admin and a writer (the writer = a deploy token to be revoked).
    let admin = admin_delegated_puzzle_from_key(&admin_key);
    let writer = writer_delegated_puzzle_from_key(&revoked_writer_key);
    let mint = mint_store(
        owner,
        vec![lead_coin(owner_ph)],
        Bytes32::new([3u8; 32]),
        None,
        None,
        None,
        None,
        owner_ph,
        vec![admin, writer],
        0,
    )
    .expect("mint with admin + writer");

    // The admin revokes the deploy token by replacing the delegated set with admin-only.
    let revoked = update_store_ownership(
        mint.new_datastore,
        owner_ph,
        vec![admin],
        DataStoreInnerSpend::Admin(admin_key),
    )
    .expect("admin revokes writer (deploy token)");
    assert!(
        !revoked.coin_spends.is_empty(),
        "admin produced a delegation-change spend"
    );
}

/// A WRITER cannot change the delegated-puzzle set (only advance the root). Guards the
/// least-privilege boundary: a deploy token can deploy but can never grant itself more authority.
#[test]
fn writer_cannot_change_delegation() {
    let owner = synthetic_from(2);
    let writer_key = synthetic_from(6);
    let owner_ph: Bytes32 = StandardArgs::curry_tree_hash(owner).into();

    let writer = writer_delegated_puzzle_from_key(&writer_key);
    let mint = mint_store(
        owner,
        vec![lead_coin(owner_ph)],
        Bytes32::new([3u8; 32]),
        None,
        None,
        None,
        None,
        owner_ph,
        vec![writer],
        0,
    )
    .expect("mint");

    let res = update_store_ownership(
        mint.new_datastore,
        owner_ph,
        vec![writer],
        DataStoreInnerSpend::Writer(writer_key),
    );
    assert!(
        matches!(res, Err(Error::Permission)),
        "writer cannot change delegation"
    );
}

/// An ORACLE-carrying store can be oracle-spent by anyone for the fixed fee (smoke test that the
/// oracle constructor wires into a working store).
#[test]
fn store_with_oracle_delegate_is_oracle_spendable() {
    use chip35_dl_coin::oracle_spend;
    let owner = synthetic_from(2);
    let owner_ph: Bytes32 = StandardArgs::curry_tree_hash(owner).into();
    let oracle = oracle_delegated_puzzle(owner_ph, 2);

    let big_coin = Coin {
        parent_coin_info: Bytes32::new([7u8; 32]),
        puzzle_hash: owner_ph,
        amount: 1000,
    };
    let mint = mint_store(
        owner,
        vec![big_coin],
        Bytes32::new([3u8; 32]),
        None,
        None,
        None,
        None,
        owner_ph,
        vec![oracle],
        0,
    )
    .expect("mint with oracle");

    let spent = oracle_spend(owner, vec![big_coin], mint.new_datastore, 0).expect("oracle spend");
    assert!(
        !spent.coin_spends.is_empty(),
        "oracle spend produced spends"
    );
}
