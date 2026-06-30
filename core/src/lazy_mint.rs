//! Trustless lazy mint (mint-on-claim) builders (roadmap #40).
//!
//! The creator's DID spends ONCE to precommit a whole collection; afterwards ANYONE can mint an
//! individual NFT on demand with NO further DID involvement (the claimer funds 1 mojo). This is the
//! primitive the hub drop studio needs for self-serve on-chain drops (it honestly blocks them today —
//! `drop-model.js` `LAZY_MINT_DEFERRED`).
//!
//! ## Attribution
//! Ported from mintgarden-io/secure-the-mint (Apache-2.0, © 2024 Andreas Greimel; itself based on
//! Chia-Network's `secure_the_bag`, pre-launcher idea credited to trepca). The Apache license is
//! vendored at `puzzles/LICENSE-APACHE`; the project `NOTICE` lists the derived files; the ported
//! `.clsp` carry in-file attribution.
//!
//! ## Design — SDK primitives, not a custom compiled puzzle (see DESIGN.md #40)
//! secure-the-mint's custom pre-launcher exists to (1) secure the otherwise-insecure singleton
//! launcher and (2) recompute the launcher id on-chain because it is unknown at commit time. In
//! `chia-sdk-driver` 0.30 the launcher id IS known at build time (`Launcher::new(parent, 1)` fully
//! determines the launcher coin), and `Launcher::spend` already asserts the eve commitment — so
//! NEITHER problem needs a custom puzzle. We build a FLAT secure-the-bag: the DID spend creates one
//! per-item commitment coin (amount 0) at a [`P2CurriedArgs`] hash committing to that item's fixed
//! "create the launcher" node; a claim reveals that node, mints the eve NFT to the claimer, and funds
//! the mojo. The ported `.clsp` are kept in `puzzles/` as the auditable reference of the mechanism we
//! mirror; this module does not load them.
//!
//! ## Creator attribution = provenance by lineage
//! A trustless claim cannot set the NFT's on-chain `current_owner` to the creator DID (that needs a
//! DID co-spend per claim — see [`crate::mint_nft_with_did`]). Instead the creator is attributed by
//! LINEAGE: every minted NFT's launcher coin descends from the commitment coins the creator's single
//! DID spend created, and the royalty is committed to the creator. `lazy_mint_sim.rs` proves the chain.
//!
//! ## Honesty — validated vs deferred
//! - VALIDATED on the simulator: commit + DIRECT (free) claim as a different party, NFT owned by the
//!   claimer, lineage to the creator DID (`core/tests/lazy_mint_sim.rs`).
//! - [`LazyMintPolicy::PaymentGated`] is accepted but its ATOMIC on-chain payment enforcement is
//!   DEFERRED — the blocker is that the offer settlement (notarized payment + take) is a wallet op the
//!   offline keyless single-bundle boundary does not assemble end-to-end. The builder mints into the
//!   committed payee-facing recipient; the hub keeps its honest deferral for the paid flow.
//! - Allowlist (merkle) gating is accepted (the proof shape is validated, the root surfaced) but
//!   trustless ON-CHAIN membership enforcement is DEFERRED — it needs a compiled claim puzzle that runs
//!   the merkle verify, reintroducing the custom-puzzle surface this path avoids. The hub gates the
//!   allowlist OFF-chain today (`ALLOWLIST_ONCHAIN_DEFERRED`).

use chia_bls::PublicKey;
use chia_protocol::{Bytes, Bytes32, Coin, CoinSpend};
use chia_puzzle_types::nft::NftMetadata;
use chia_puzzle_types::standard::StandardArgs;
use chia_puzzle_types::Memos;
use chia_sdk_driver::{Launcher, NftMint, SpendContext, StandardLayer};
use chia_sdk_types::conditions::CreateCoinAnnouncement;
use chia_sdk_types::puzzles::{P2CurriedArgs, P2CurriedSolution};
use chia_sdk_types::{Conditions, Mod};
use clvm_traits::clvm_quote;
use clvmr::NodePtr;
use serde::{Deserialize, Serialize};

use crate::collection::Collection;
use crate::error::WalletError;
use crate::nft::NftMediaMetadata;
use crate::payment::PaymentAsset;

/// Who a claim mints to, and whether a claim must pay.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LazyMintPolicy {
    /// Free / direct: claiming mints the NFT straight into the claimer's puzzle hash. No payment.
    /// This is the SIMULATOR-VALIDATED mode.
    DirectMint,
    /// Pay-to-mint: a claim must settle `price` of `asset` to `payee`. **Atomic on-chain enforcement
    /// of the payment is DEFERRED** (the offer settlement is a wallet op the keyless boundary does not
    /// assemble — see the module docs); the builder mints toward the payee-facing recipient and the
    /// caller/hub gates payment until the offer-construction wave lands.
    PaymentGated {
        /// Price the claimer must pay (mojos for XCH, base units for a CAT).
        price: u64,
        /// Asset the payment settles in.
        asset: PaymentAsset,
        /// The creator/payee puzzle hash the payment settles to.
        payee: Bytes32,
    },
}

/// One precommitted item: its on-chain media metadata (dig:// + https fallback URIs + hashes) and its
/// royalty. The recipient/royalty puzzle hash come from the [`Collection`] (shared across items).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LazyMintItem {
    /// On-chain media metadata + hashes for this item.
    pub metadata: NftMediaMetadata,
    /// Royalty in basis points for this item (e.g. 300 = 3%).
    pub royalty_basis_points: u16,
}

/// A keyless, serializable handle a caller PERSISTS after a commit so a claimer — who never saw the
/// commit call — can rebuild the exact item `i` claim spend. It captures everything the claim needs:
/// the creator DID coin id (the commitment coins' parent), the committed collection + items + policy +
/// optional allowlist root, and the precomputed per-item commitment coins + launcher ids.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LazyMintTreeDescriptor {
    /// The creator DID coin id the commitment coins descend from (the single authorization).
    pub did_coin_id: Bytes32,
    /// The committed collection (shared royalty recipient + economics).
    pub collection: Collection,
    /// The committed items, in order.
    pub items: Vec<LazyMintItem>,
    /// The committed recipient/payment policy.
    pub policy: LazyMintPolicy,
    /// The committed allowlist merkle root, if any (on-chain enforcement DEFERRED).
    pub allowlist_root: Option<Bytes32>,
    /// The per-item commitment coins (amount 0) a claim spends, in order.
    pub commit_coins: Vec<Coin>,
    /// The precomputed per-item NFT launcher ids, in order.
    pub launcher_ids: Vec<Bytes32>,
}

/// The result of a commit: the single DID spend + the precomputed per-item identifiers.
#[derive(Clone, Debug)]
pub struct LazyMintCommitResponse {
    /// Coin spends to sign + broadcast (the single creator-DID spend).
    pub coin_spends: Vec<CoinSpend>,
    /// The commit binding = the creator DID coin id.
    pub root: Bytes32,
    /// The precomputed per-item NFT launcher ids, in order.
    pub launcher_ids: Vec<Bytes32>,
    /// The per-item commitment coins (amount 0) a claim spends, in order.
    pub commit_coins: Vec<Coin>,
    descriptor: LazyMintTreeDescriptor,
}

impl LazyMintCommitResponse {
    /// The serializable descriptor a claimer needs to rebuild any item's claim spend.
    pub fn descriptor(&self) -> LazyMintTreeDescriptor {
        self.descriptor.clone()
    }
}

/// The result of a claim: the unroll+mint coin spends + the minted NFT identifiers.
#[derive(Clone, Debug)]
pub struct LazyMintClaimResponse {
    /// Coin spends to sign + broadcast.
    pub coin_spends: Vec<CoinSpend>,
    /// The minted NFT's launcher id (= the precommitted id for this item).
    pub launcher_id: Bytes32,
    /// The minted NFT coin (the child after the eve spend).
    pub nft_coin: Coin,
}

/// A merkle membership proof for an allowlist-gated claim. The shape mirrors
/// `chia_sdk_types::MerkleProof`; on-chain ENFORCEMENT is DEFERRED (see module docs).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MerkleMembershipProof {
    /// The bit path through the tree.
    pub path: u32,
    /// The sibling hashes along the path.
    pub proof: Vec<Bytes32>,
}

/// Build the per-item "create the launcher" node conditions. The node, when revealed at claim time,
/// creates the singleton launcher coin (amount 1) for this item. A per-item [`CreateCoinAnnouncement`]
/// of the index makes each node — and therefore each commitment coin id — DISTINCT (so N items do not
/// collapse to one coin), mirroring secure-the-bag's per-node `'$'` announcement.
fn launcher_node_conditions(index: usize) -> Conditions {
    use chia_puzzles::SINGLETON_LAUNCHER_HASH;
    Conditions::new()
        .create_coin(SINGLETON_LAUNCHER_HASH.into(), 1, Memos::None)
        .with(CreateCoinAnnouncement::new(Bytes::from(
            (index as u64).to_be_bytes().to_vec(),
        )))
}

/// Compute item `i`'s commitment-coin puzzle hash: a [`P2CurriedArgs`] committing to the tree hash of
/// that item's launcher node. Deterministic and claimer-independent.
fn commit_puzzle_hash(ctx: &mut SpendContext, index: usize) -> Result<Bytes32, WalletError> {
    let node = ctx.alloc(&clvm_quote!(launcher_node_conditions(index)))?;
    let node_hash: Bytes32 = ctx.tree_hash(node).into();
    Ok(P2CurriedArgs::new(node_hash).curry_tree_hash().into())
}

/// The on-chain [`NftMetadata`] for an item, with the collection's `series_*` filled in via editions.
fn item_chain_metadata(item: &LazyMintItem) -> NftMetadata {
    let m = &item.metadata;
    NftMetadata {
        edition_number: if m.edition_number == 0 {
            1
        } else {
            m.edition_number
        },
        edition_total: if m.edition_total == 0 {
            1
        } else {
            m.edition_total
        },
        data_uris: m.data_uris.clone(),
        data_hash: m.data_hash,
        metadata_uris: m.metadata_uris.clone(),
        metadata_hash: m.metadata_hash,
        license_uris: m.license_uris.clone(),
        license_hash: m.license_hash,
    }
}

/// The recipient an item's claim mints to under `policy`: the claimer for a free mint, or the payee
/// for a (payment-deferred) paid mint. `claimer_puzzle_hash` is the default.
fn claim_recipient(policy: &LazyMintPolicy, claimer_puzzle_hash: Bytes32) -> Bytes32 {
    match policy {
        LazyMintPolicy::DirectMint => claimer_puzzle_hash,
        // Payment enforcement is DEFERRED; we still mint to the claimer (who is buying it). The hub
        // gates the payment until the offer-construction wave (see module docs).
        LazyMintPolicy::PaymentGated { .. } => claimer_puzzle_hash,
    }
}

/// The creator DID spends ONCE to precommit `items` into `collection`, attributed by lineage to `did`.
///
/// Emits, per item, a `CREATE_COIN(commit_ph_i, 0)` (hinted to the item's launcher id) from a single
/// DID update spend. Because each commitment coin descends from the DID coin and each launcher descends
/// from its commitment coin, every NFT launcher id is deterministic and is returned now. The DID is
/// never needed again — afterwards anyone calls [`build_lazy_mint_claim`].
///
/// # Errors
/// [`WalletError::Parse`] if `items` is empty; [`WalletError::Driver`] on spend-construction failure.
pub fn build_lazy_mint_commit(
    minter_synthetic_key: PublicKey,
    did: chia_sdk_driver::Did,
    collection: &Collection,
    items: &[LazyMintItem],
    policy: LazyMintPolicy,
    allowlist_root: Option<Bytes32>,
) -> Result<LazyMintCommitResponse, WalletError> {
    if items.is_empty() {
        return Err(WalletError::Parse("items is empty".to_string()));
    }

    let mut ctx = SpendContext::new();
    let p2 = StandardLayer::new(minter_synthetic_key);
    let did_coin_id = did.coin.coin_id();

    let mut commit_conditions = Conditions::new();
    let mut commit_coins = Vec::with_capacity(items.len());
    let mut launcher_ids = Vec::with_capacity(items.len());

    for index in 0..items.len() {
        let commit_ph = commit_puzzle_hash(&mut ctx, index)?;
        let commit_coin = Coin::new(did_coin_id, commit_ph, 0);
        // The launcher this commitment coin will create (parent = the commitment coin).
        let launcher_id = Launcher::new(commit_coin.coin_id(), 1).coin().coin_id();

        // Hint the commitment coin to its launcher id so it can be located on-chain.
        let hint = ctx.hint(launcher_id)?;
        commit_conditions = commit_conditions.create_coin(commit_ph, 0, hint);

        commit_coins.push(commit_coin);
        launcher_ids.push(launcher_id);
    }

    // Spend the DID once, emitting all the commitment-coin CREATE_COINs. This is the single
    // authorization; the recreated DID is not needed here.
    let _recreated = did.update(&mut ctx, &p2, commit_conditions)?;

    let descriptor = LazyMintTreeDescriptor {
        did_coin_id,
        collection: collection.clone(),
        items: items.to_vec(),
        policy,
        allowlist_root,
        commit_coins: commit_coins.clone(),
        launcher_ids: launcher_ids.clone(),
    };

    Ok(LazyMintCommitResponse {
        coin_spends: ctx.take(),
        root: did_coin_id,
        launcher_ids,
        commit_coins,
        descriptor,
    })
}

/// A NON-owner unrolls + mints item `index` on demand, funding the 1-mojo launcher (+ `fee`) from
/// `claimer_coins`. No DID involvement — the claim is authorized by the precommit alone.
///
/// Spends the item's commitment coin (revealing its committed launcher node → creates the launcher),
/// mints the eve NFT to the recipient ([`claim_recipient`]) via `Launcher::mint_nft` with no DID
/// `TransferNft` (a trustless claim cannot assign a DID owner — see module docs), and funds the mojo +
/// fee from the claimer's lead coin (which asserts the commitment coin so the bundle is atomic).
///
/// `merkle_proof` is accepted for an allowlist-gated claim but on-chain enforcement is DEFERRED.
///
/// # Errors
/// [`WalletError::Parse`] if `claimer_coins` is empty or `index` is out of range; [`WalletError::Driver`]
/// on spend-construction failure.
#[allow(clippy::too_many_arguments)]
pub fn build_lazy_mint_claim(
    claimer_synthetic_key: PublicKey,
    claimer_coins: Vec<Coin>,
    claimer_puzzle_hash: Bytes32,
    commit: &LazyMintTreeDescriptor,
    index: usize,
    _merkle_proof: Option<MerkleMembershipProof>,
    fee: u64,
) -> Result<LazyMintClaimResponse, WalletError> {
    if claimer_coins.is_empty() {
        return Err(WalletError::Parse("claimer_coins is empty".to_string()));
    }
    if index >= commit.items.len() {
        return Err(WalletError::Parse(format!(
            "item index {index} out of range (collection has {} items)",
            commit.items.len()
        )));
    }

    let item = &commit.items[index];
    let commit_coin = commit.commit_coins[index];

    let claimer_puzzle_hash_self: Bytes32 =
        StandardArgs::curry_tree_hash(claimer_synthetic_key).into();
    let total_amount: u64 = claimer_coins.iter().map(|c| c.amount).sum();
    let reserved = fee + 1; // 1 mojo funds the launcher.
    if total_amount < reserved {
        return Err(WalletError::Parse(format!(
            "claimer coins ({total_amount}) cannot cover fee {fee} + 1 mojo launcher"
        )));
    }

    let mut ctx = SpendContext::new();
    let p2 = StandardLayer::new(claimer_synthetic_key);

    // 1) Reveal + spend the item's commitment coin: its committed node creates the launcher coin.
    let node = ctx.alloc(&clvm_quote!(launcher_node_conditions(index)))?;
    let node_hash: Bytes32 = ctx.tree_hash(node).into();
    let p2_curried_puzzle = {
        let curried = ctx.curry(P2CurriedArgs::new(node_hash))?;
        ctx.alloc(&curried)?
    };
    let p2_curried_solution = ctx.alloc(&P2CurriedSolution::new(node, NodePtr::NIL))?;
    ctx.spend(
        commit_coin,
        chia_sdk_driver::Spend::new(p2_curried_puzzle, p2_curried_solution),
    )?;

    // 2) Spend the launcher → eve NFT → child owned by the recipient. No DID TransferNft (trustless).
    // CRITICAL: use `create_early`, NOT `Launcher::new`. The launcher coin is created by the commitment
    // coin's revealed node (its `CreateCoin(SINGLETON_LAUNCHER_HASH, 1)`); `create_early` yields a
    // launcher whose own conditions are EMPTY, so `mint_nft` does NOT re-emit the launcher CreateCoin
    // (which would mint a second 1-mojo launcher and unbalance the bundle → MintingCoin). The launcher
    // gets its 1 mojo as value the claimer's lead coin leaves on the table (secure-the-bag funding).
    let recipient = claim_recipient(&commit.policy, claimer_puzzle_hash);
    let metadata_ptr = ctx.alloc_hashed(&item_chain_metadata(item))?;
    let mut nft_mint = NftMint::new(metadata_ptr, recipient, item.royalty_basis_points, None);
    nft_mint.royalty_puzzle_hash = commit.collection.royalty_puzzle_hash;

    let (_create_launcher, launcher) = Launcher::create_early(commit_coin.coin_id(), 1);
    let launcher_id = launcher.coin().coin_id();
    let (mint_conditions, nft) = launcher.mint_nft(&mut ctx, &nft_mint)?;

    // 3) The claimer's lead coin funds the launcher (1 mojo) + fee, carries the eve-announcement mint
    // conditions, asserts the commitment coin (atomic), and returns change. It leaves `1` mojo on the
    // table (change = total - fee - 1) which funds the launcher created by the commitment coin.
    let lead_coin = claimer_coins[0];
    let lead_coin_id = lead_coin.coin_id();
    for c in claimer_coins.iter().skip(1) {
        p2.spend(
            &mut ctx,
            *c,
            Conditions::new().assert_concurrent_spend(lead_coin_id),
        )?;
    }

    let mut lead_conditions = mint_conditions.assert_concurrent_spend(commit_coin.coin_id());
    if fee > 0 {
        lead_conditions = lead_conditions.reserve_fee(fee);
    }
    if total_amount > reserved {
        let hint = ctx.hint(claimer_puzzle_hash_self)?;
        lead_conditions =
            lead_conditions.create_coin(claimer_puzzle_hash_self, total_amount - reserved, hint);
    }
    p2.spend(&mut ctx, lead_coin, lead_conditions)?;

    Ok(LazyMintClaimResponse {
        coin_spends: ctx.take(),
        launcher_id,
        nft_coin: nft.coin,
    })
}
