//! Deploy tokens (roadmap #17) — the REAL model: a deploy token is a **revocable writer delegate**.
//!
//! The prior scaffold (`core/src/deploy_token.rs`, a bespoke curried puzzle pending security
//! review) is superseded. There is no special deploy-token type. The CHIP-0035 DataStore delegation
//! layer already gives us exactly the least-privilege deploy credential we wanted:
//!
//! - **Issue**: the owner adds `writer_delegated_puzzle_from_key(ci_deploy_key)` to the store's
//!   delegated-puzzle set via [`update_store_ownership`].
//! - **Deploy**: the CI deploy key advances the root with [`update_store_metadata`] under
//!   [`DataStoreInnerSpend::Writer`] — a root advance IS a metadata update — WITHOUT the owner seed.
//! - **Revoke**: the owner (or an admin delegate) replaces the delegated-puzzle set, dropping the
//!   writer; the revoked key can no longer advance the store.
//!
//! A writer delegate cannot change delegation or transfer ownership, so a deploy token can deploy
//! but can never escalate its own authority. The one non-native extra — an on-chain DIG **spend
//! cap** bounding cumulative spend per token — is future work (see DESIGN.md → #17).

use chia_puzzle_types::{standard::StandardArgs, DeriveSynthetic};
use chip35_dl_coin::{
    master_to_wallet_unhardened, mint_store, update_store_metadata, update_store_ownership,
    writer_delegated_puzzle_from_key, Bytes32, Coin, DataStoreInnerSpend, Error, SecretKey,
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

/// ISSUE → DEPLOY → REVOKE: the full deploy-token lifecycle as writer delegation.
#[test]
fn deploy_token_issue_advance_revoke_via_writer_delegate() {
    let owner = synthetic_from(2);
    let deploy_key = synthetic_from(6); // CI deploy key — NEVER the owner/master seed
    let owner_ph: Bytes32 = StandardArgs::curry_tree_hash(owner).into();

    // Mint owner-only.
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

    // ISSUE: owner adds the deploy key as a writer.
    let token = writer_delegated_puzzle_from_key(&deploy_key);
    let issued = update_store_ownership(
        mint.new_datastore,
        owner_ph,
        vec![token],
        DataStoreInnerSpend::Owner(owner),
    )
    .expect("owner issues deploy token");
    assert_eq!(
        issued.new_datastore.info.delegated_puzzles,
        vec![token],
        "store carries the deploy token (writer delegate)"
    );

    // DEPLOY: the deploy key advances the root — no owner seed.
    let deployed = update_store_metadata(
        issued.new_datastore.clone(),
        Bytes32::new([9u8; 32]),
        None,
        None,
        None,
        None,
        DataStoreInnerSpend::Writer(deploy_key),
    )
    .expect("deploy key advances root");
    assert_eq!(
        deployed.new_datastore.info.metadata.root_hash,
        Bytes32::new([9u8; 32]),
        "deploy advanced the store to a new capsule"
    );

    // REVOKE: owner replaces the delegated set, dropping the writer.
    let revoked = update_store_ownership(
        deployed.new_datastore,
        owner_ph,
        vec![],
        DataStoreInnerSpend::Owner(owner),
    )
    .expect("owner revokes deploy token");
    assert!(
        revoked.new_datastore.info.delegated_puzzles.is_empty(),
        "deploy token revoked — no delegates remain"
    );
}

/// A deploy token (writer) cannot change delegation — it can deploy but never escalate.
#[test]
fn deploy_token_cannot_escalate() {
    let owner = synthetic_from(2);
    let deploy_key = synthetic_from(6);
    let owner_ph: Bytes32 = StandardArgs::curry_tree_hash(owner).into();

    let token = writer_delegated_puzzle_from_key(&deploy_key);
    let mint = mint_store(
        owner,
        vec![lead_coin(owner_ph)],
        Bytes32::new([3u8; 32]),
        None,
        None,
        None,
        None,
        owner_ph,
        vec![token],
        0,
    )
    .expect("mint with deploy token");

    // The deploy key tries to grant itself (change the delegated set) — must be denied.
    let res = update_store_ownership(
        mint.new_datastore,
        owner_ph,
        vec![token],
        DataStoreInnerSpend::Writer(deploy_key),
    );
    assert!(
        matches!(res, Err(Error::Permission)),
        "a deploy token cannot change delegation"
    );
}
