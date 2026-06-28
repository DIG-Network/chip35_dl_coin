//! Scoped, revocable, spend-capped deploy-token delegation (roadmap #17).
//!
//! # ⚠️ SCAFFOLD — PENDING SECURITY REVIEW. NOT IMPLEMENTED.
//!
//! This is the design-of-record interface for a store-bound, spend-capped, revocable delegation
//! that authorizes on-chain root advances WITHOUT the master seed — the safe CI-auth model for
//! auto-deploy (roadmap #7/#23). It is a **security-critical on-chain authorization puzzle**.
//!
//! Per the task constraints, the CLVM puzzle and the spend-builder bodies are intentionally NOT
//! implemented here; they require security review before any real authorization code is written.
//! Every function below returns [`WalletError::Parse`] with a "scaffold pending review" message, and
//! the accompanying tests in `core/tests/deploy_token.rs` are `#[ignore]`d with the same reason so
//! they document intended behavior without asserting it against unwritten auth code.
//!
//! The wasm boundary deliberately exposes **none** of this, so nothing downstream can depend on
//! unreviewed auth code. See `DESIGN.md` → "#17" for the full model and open questions.

use chia_bls::PublicKey;
use chia_protocol::Bytes32;
use chia_sdk_driver::{DataStore, DelegatedPuzzle};

use crate::error::WalletError;
use crate::types::SuccessResponse;

/// Sentinel error message every scaffold function returns until #17 passes security review.
pub const SCAFFOLD_PENDING_REVIEW: &str =
    "deploy-token delegation is a scaffold pending security review (roadmap #17)";

/// The curried terms that bind and constrain a deploy token. All limits are intended to be enforced
/// ON-CHAIN by the delegation puzzle, not merely checked off-chain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeployTokenTerms {
    /// The store this token may advance — and ONLY this store. Curried into the puzzle so a token
    /// for store A cannot touch store B.
    pub store_launcher_id: Bytes32,
    /// The CI deploy key authorized to sign root advances under this token. NOT the master seed.
    pub authorized_public_key: PublicKey,
    /// Maximum cumulative mojos the token may spend across all advances (the spend cap).
    pub max_total_mojos: u64,
    /// Optional cap on the number of root advances (0 = unlimited advances within the other limits).
    pub max_advances: u32,
    /// Absolute unix-seconds expiry; after this the puzzle self-rejects
    /// (`ASSERT_BEFORE_SECONDS_ABSOLUTE`).
    pub expires_at_seconds: u64,
}

/// **SCAFFOLD (pending review).** Build the delegated-puzzle entry the owner adds to the store's
/// delegated-puzzle set so the deploy key can advance the root within the curried limits.
///
/// Intended behavior (see tests): produce a `DelegatedPuzzle` curried with [`DeployTokenTerms`]
/// (store binding + spend cap + advance cap + expiry), addable via the existing
/// [`crate::update_store_ownership`] delegated-puzzle replacement path, and revocable by replacing
/// that set.
///
/// # Errors
/// Always returns [`WalletError::Parse`] ([`SCAFFOLD_PENDING_REVIEW`]) until #17 is implemented.
pub fn build_deploy_token(_terms: &DeployTokenTerms) -> Result<DelegatedPuzzle, WalletError> {
    Err(WalletError::Parse(SCAFFOLD_PENDING_REVIEW.to_string()))
}

/// **SCAFFOLD (pending review).** Build the root-advance spend signed by the deploy key, asserting
/// the curried cap + expiry on-chain.
///
/// Intended behavior (see tests): advance `datastore` to `new_root_hash` authorized by the deploy
/// token (not the owner key), with the puzzle asserting the spend stays within
/// `terms.max_total_mojos` / `terms.max_advances` and before `terms.expires_at_seconds`. Reuses the
/// writer-filter (metadata-only) authority — a root advance IS a metadata update — curried with the
/// extra asserts. Returns the new [`DataStore`] state like the other update builders.
///
/// # Errors
/// Always returns [`WalletError::Parse`] ([`SCAFFOLD_PENDING_REVIEW`]) until #17 is implemented.
pub fn deploy_token_advance_root(
    _datastore: DataStore,
    _new_root_hash: Bytes32,
    _terms: &DeployTokenTerms,
) -> Result<SuccessResponse, WalletError> {
    Err(WalletError::Parse(SCAFFOLD_PENDING_REVIEW.to_string()))
}
