//! Tests for the scoped deploy-token delegation (roadmap #17).
//!
//! # SCAFFOLD — these document INTENDED behavior and are `#[ignore]`d pending security review.
//!
//! The deploy-token puzzle is security-critical on-chain authorization and is intentionally NOT
//! implemented (see `core/src/deploy_token.rs` + `DESIGN.md`). Each behavioral test is ignored with
//! a "scaffold pending review" reason so it describes the target contract without gating CI on
//! unwritten auth code. The one non-ignored test asserts the scaffold's fail-closed stance: until
//! reviewed, every entry point refuses with [`SCAFFOLD_PENDING_REVIEW`].

use chia_puzzle_types::DeriveSynthetic;
use chip35_dl_coin::{
    build_deploy_token, deploy_token_advance_root, master_to_wallet_unhardened, Bytes32,
    DeployTokenTerms, Error, SecretKey, SCAFFOLD_PENDING_REVIEW,
};

fn terms() -> DeployTokenTerms {
    let sk = SecretKey::from_seed(&[9u8; 32]);
    let ci_key = master_to_wallet_unhardened(&sk.public_key(), 0).derive_synthetic();
    DeployTokenTerms {
        store_launcher_id: Bytes32::new([1u8; 32]),
        authorized_public_key: ci_key,
        max_total_mojos: 500_000_000_000, // ~5 deploys worth of DIG at the keyless boundary
        max_advances: 5,
        expires_at_seconds: 4_000_000_000,
    }
}

/// FAIL-CLOSED (active): until #17 is reviewed+implemented, the scaffold must refuse, never silently
/// "authorize" anything. This guards against the scaffold accidentally shipping as a no-op allow.
#[test]
fn scaffold_is_fail_closed_until_reviewed() {
    let t = terms();
    let build = build_deploy_token(&t);
    assert!(
        matches!(&build, Err(Error::Parse(m)) if m == SCAFFOLD_PENDING_REVIEW),
        "build_deploy_token must fail closed with the scaffold sentinel"
    );
    // deploy_token_advance_root needs a DataStore; constructing one is part of the (ignored)
    // behavioral test below. Here we only assert the function exists and is wired (compile gate).
    let _ = deploy_token_advance_root;
}

// ---- Intended behavior (ignored: scaffold pending security review) ----

#[test]
#[ignore = "scaffold pending security review (#17): deploy-token puzzle not yet implemented"]
fn build_deploy_token_binds_to_one_store() {
    // INTENT: the returned DelegatedPuzzle is curried with terms.store_launcher_id so it can only
    // authorize spends of THAT singleton; a token built for store A must be rejected when used
    // against store B's singleton. Verify via the simulator once the puzzle exists.
    let t = terms();
    let _dp = build_deploy_token(&t).expect("deploy token builds");
    unimplemented!("verify store-binding rejection against a different launcher_id");
}

#[test]
#[ignore = "scaffold pending security review (#17): spend cap not yet enforced"]
fn deploy_token_enforces_spend_cap() {
    // INTENT: an advance that would push cumulative spend over terms.max_total_mojos (or exceed
    // terms.max_advances) is rejected on-chain. Verify via the simulator.
    unimplemented!("verify cap enforcement");
}

#[test]
#[ignore = "scaffold pending security review (#17): expiry not yet enforced"]
fn deploy_token_self_expires() {
    // INTENT: after terms.expires_at_seconds the puzzle self-rejects (ASSERT_BEFORE_SECONDS_ABSOLUTE).
    unimplemented!("verify expiry");
}

#[test]
#[ignore = "scaffold pending security review (#17): revocation not yet wired"]
fn owner_can_revoke_deploy_token() {
    // INTENT: the owner replaces the store's delegated-puzzle set (via update_store_ownership),
    // invalidating the token immediately; a subsequent advance with the revoked token is rejected.
    unimplemented!("verify revocation via delegated-set replacement");
}

#[test]
#[ignore = "scaffold pending security review (#17): root advance not yet implemented"]
fn deploy_token_advances_root_without_master_seed() {
    // INTENT: deploy_token_advance_root advances the store root signed ONLY by terms.authorized
    // CI key (never the owner/master seed), returning the new DataStore state.
    unimplemented!("verify keyless-of-master root advance");
}
