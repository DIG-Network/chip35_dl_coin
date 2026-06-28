//! NFT-gating: PROVE NFT / collection ownership so a dapp can gate access on it (roadmap #46).
//!
//! Read/verify only — there is NO spend here. Given the coin spend that created an NFT's CURRENT coin
//! (fetched from a full node by the caller) and a claimed owner, this reconstructs the NFT and checks:
//! who owns it (the inner p2 puzzle hash = the owner's address), which collection/creator it is
//! attributed to (the singleton's `current_owner`, i.e. the creator DID), and its launcher id. A dapp
//! gates by asserting `owner == the connected wallet` AND (optionally) `attributed_did == the
//! required collection DID` / `launcher_id == a specific required NFT`.
//!
//! Why a coin spend, not a coin: an NFT's CURRENT owner + attribution live in the inner puzzle, which
//! is only revealed when the coin is spent. The caller fetches the spend that produced the latest
//! (unspent) NFT coin — `parse_child` reconstructs that latest coin's [`NftInfo`] from its parent
//! spend. Confirming the resulting coin is unspent (i.e. the NFT is still held) is the caller's job
//! (a coinset `get_coin_record_by_name` on [`NftOwnershipProof::nft_coin_id`]); this module proves
//! the on-chain FACTS, the caller pairs them with liveness.

use chia_protocol::{Bytes32, CoinSpend};
use chia_sdk_driver::{Nft, Puzzle, SpendContext};

/// The facts a successful proof establishes about an NFT (all read from the chain, none asserted).
///
/// A dapp turns these into a gate: require `owner_puzzle_hash == connected wallet`, and optionally
/// `attributed_did == required collection/creator DID` and/or `launcher_id == a specific NFT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NftOwnershipProof {
    /// The NFT's launcher id (its permanent identifier).
    pub launcher_id: Bytes32,
    /// The current owner's puzzle hash (the NFT's inner p2 puzzle hash = the holder's address).
    pub owner_puzzle_hash: Bytes32,
    /// The singleton this NFT is attributed to (the creator/collection DID launcher id), if any. An
    /// unassigned NFT (e.g. mid-transfer or in an offer) has `None`.
    pub attributed_did: Option<Bytes32>,
    /// The current (unspent) NFT coin id — the caller checks it is still unspent for liveness.
    pub nft_coin_id: Bytes32,
}

/// The reasons an NFT gate can fail — actionable so a dapp can tell the user why access was denied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GatingError {
    /// The supplied coin spend is not an NFT spend (cannot reconstruct an NFT from it).
    NotAnNft,
    /// The NFT is held by a different puzzle hash than the claimed owner.
    WrongOwner { expected: Bytes32, got: Bytes32 },
    /// The NFT is not attributed to the required collection/creator DID.
    WrongCollection {
        required: Bytes32,
        got: Option<Bytes32>,
    },
    /// The NFT's launcher id is not the required one.
    WrongNft { required: Bytes32, got: Bytes32 },
}

impl core::fmt::Display for GatingError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            GatingError::NotAnNft => write!(f, "coin spend is not an NFT spend"),
            GatingError::WrongOwner { expected, got } => {
                write!(f, "NFT owner {got} is not the claimed owner {expected}")
            }
            GatingError::WrongCollection { required, got } => {
                write!(f, "NFT collection {got:?} is not the required {required}")
            }
            GatingError::WrongNft { required, got } => {
                write!(f, "NFT {got} is not the required {required}")
            }
        }
    }
}

/// Reconstruct the CURRENT [`NftOwnershipProof`] for an NFT from the coin spend that created its
/// latest coin.
///
/// `parent_spend` is the spend of the NFT's PARENT coin (the spend that produced the current unspent
/// NFT coin) — the caller fetches it from a full node. Returns the read facts (owner, attributed DID,
/// launcher id, current coin id) or [`GatingError::NotAnNft`] if the spend isn't an NFT.
///
/// This does no gating itself; use [`prove_nft_ownership`] / [`prove_collection_membership`] to apply
/// a require check, or read the facts and decide in the dapp.
pub fn read_nft_ownership(parent_spend: &CoinSpend) -> Result<NftOwnershipProof, GatingError> {
    let mut ctx = SpendContext::new();
    let puzzle_ptr = ctx
        .alloc(&parent_spend.puzzle_reveal)
        .map_err(|_| GatingError::NotAnNft)?;
    let solution_ptr = ctx
        .alloc(&parent_spend.solution)
        .map_err(|_| GatingError::NotAnNft)?;
    let puzzle = Puzzle::parse(&ctx, puzzle_ptr);

    let nft = Nft::parse_child(&mut ctx, parent_spend.coin, puzzle, solution_ptr)
        .map_err(|_| GatingError::NotAnNft)?
        .ok_or(GatingError::NotAnNft)?;

    Ok(NftOwnershipProof {
        launcher_id: nft.info.launcher_id,
        owner_puzzle_hash: nft.info.p2_puzzle_hash,
        attributed_did: nft.info.current_owner,
        nft_coin_id: nft.coin.coin_id(),
    })
}

/// Prove an NFT is owned by `claimed_owner_puzzle_hash` (roadmap #46 NFT-gating).
///
/// Reconstructs the NFT from `parent_spend` (see [`read_nft_ownership`]) and asserts its current
/// holder is `claimed_owner_puzzle_hash`. When `required_nft` is set, also asserts the NFT's launcher
/// id matches (gate on a SPECIFIC NFT). Returns the proof facts on success.
///
/// The caller still confirms [`NftOwnershipProof::nft_coin_id`] is unspent on-chain (liveness) before
/// granting access.
pub fn prove_nft_ownership(
    parent_spend: &CoinSpend,
    claimed_owner_puzzle_hash: Bytes32,
    required_nft: Option<Bytes32>,
) -> Result<NftOwnershipProof, GatingError> {
    let proof = read_nft_ownership(parent_spend)?;
    if proof.owner_puzzle_hash != claimed_owner_puzzle_hash {
        return Err(GatingError::WrongOwner {
            expected: claimed_owner_puzzle_hash,
            got: proof.owner_puzzle_hash,
        });
    }
    if let Some(required) = required_nft {
        if proof.launcher_id != required {
            return Err(GatingError::WrongNft {
                required,
                got: proof.launcher_id,
            });
        }
    }
    Ok(proof)
}

/// Prove an NFT held by `claimed_owner_puzzle_hash` is a member of the collection/creator identified
/// by `required_did` (roadmap #46 collection-gating).
///
/// Combines the owner check with an attribution check: the NFT's `current_owner` (its creator/
/// collection DID) must equal `required_did`. This is the "gate on collection membership" path — a
/// dapp requires holders of any NFT minted under a given creator DID. Returns the proof facts.
///
/// NOTE: this proves attribution to the creator DID the NFT carries on-chain (`current_owner`), which
/// is how DIG mints attribute a collection (see [`crate::collection`] / [`crate::nft::DidAttribution`]).
/// Verifying the DID itself authorized that attribution (vs. a spoofed launcher id) is a deeper check
/// the caller layers on by also proving the DID — out of scope for this read helper.
pub fn prove_collection_membership(
    parent_spend: &CoinSpend,
    claimed_owner_puzzle_hash: Bytes32,
    required_did: Bytes32,
) -> Result<NftOwnershipProof, GatingError> {
    let proof = prove_nft_ownership(parent_spend, claimed_owner_puzzle_hash, None)?;
    if proof.attributed_did != Some(required_did) {
        return Err(GatingError::WrongCollection {
            required: required_did,
            got: proof.attributed_did,
        });
    }
    Ok(proof)
}
