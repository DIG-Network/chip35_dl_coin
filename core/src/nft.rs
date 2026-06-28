//! NFT mint spend builders (roadmap #33 / #35).
//!
//! Builds the keyless coin spends that mint a single NFT — including the DIG-capsule media path
//! where the NFT's `data_uris` / `metadata_uris` point at a `dig://` URN (with an https gateway
//! fallback URI) and the hashes are computed from the real bytes (via [`crate::metadata`]).
//!
//! This crate does NOT build the capsule that stores the media — that is `digstore`. It exposes the
//! spend builder that takes the already-computed hashes + URIs and mints the NFT. URI ordering
//! (dig:// first, https fallback second) is a toolkit convention; the builder accepts whatever list
//! the caller passes.

use chia_bls::PublicKey;
use chia_protocol::{Bytes32, Coin, CoinSpend};
use chia_puzzle_types::nft::NftMetadata;
use chia_puzzle_types::standard::StandardArgs;
use chia_sdk_driver::{Launcher, NftMint, SpendContext, StandardLayer};
use chia_sdk_types::{conditions::TransferNft, Conditions};

use crate::error::WalletError;

/// DID attribution for a mint: ties the NFT to a creator-identity DID so collectors can verify who
/// minted it (roadmap #38's building block). The DID owner must authorize the mint elsewhere in the
/// bundle (the DID spend); here it only supplies the on-chain `TransferNft` condition.
#[derive(Clone, Debug)]
pub struct DidAttribution {
    /// The creator DID's launcher id.
    pub launcher_id: Bytes32,
    /// The DID's current inner puzzle hash (proves the DID acknowledged the mint).
    pub inner_puzzle_hash: Bytes32,
}

/// The on-chain NFT metadata fields, with hashes already computed from the real bytes (#36).
///
/// `*_uris` are ordered by the caller; the DIG toolkit puts the `dig://` URN first and an https
/// gateway URL second so a verifier prefers the permanent capsule and falls back to the gateway.
#[derive(Clone, Debug, Default)]
pub struct NftMediaMetadata {
    /// URIs serving the primary media (dig:// first, https fallback second by convention).
    pub data_uris: Vec<String>,
    /// `sha256(media_bytes)` — pinned on-chain, MUST match what `data_uris` serve.
    pub data_hash: Option<Bytes32>,
    /// URIs serving the CHIP-0007 metadata JSON.
    pub metadata_uris: Vec<String>,
    /// `sha256(metadata_json_bytes)` — pinned on-chain.
    pub metadata_hash: Option<Bytes32>,
    /// URIs serving the license document.
    pub license_uris: Vec<String>,
    /// `sha256(license_bytes)` — pinned on-chain.
    pub license_hash: Option<Bytes32>,
    /// 1-based edition number (defaults to 1).
    pub edition_number: u64,
    /// Total editions (defaults to 1).
    pub edition_total: u64,
}

impl NftMediaMetadata {
    /// Convert into the on-chain [`NftMetadata`] CLVM struct.
    fn to_chain(&self) -> NftMetadata {
        NftMetadata {
            edition_number: if self.edition_number == 0 {
                1
            } else {
                self.edition_number
            },
            edition_total: if self.edition_total == 0 {
                1
            } else {
                self.edition_total
            },
            data_uris: self.data_uris.clone(),
            data_hash: self.data_hash,
            metadata_uris: self.metadata_uris.clone(),
            metadata_hash: self.metadata_hash,
            license_uris: self.license_uris.clone(),
            license_hash: self.license_hash,
        }
    }
}

/// Everything needed to mint one NFT (a single-item mint; bulk mint is in [`crate::collection`]).
#[derive(Clone, Debug)]
pub struct NftMintParams {
    /// The on-chain media metadata + hashes (dig:// + https fallback URIs).
    pub metadata: NftMediaMetadata,
    /// The puzzle hash that will own the minted NFT (the recipient).
    pub p2_puzzle_hash: Bytes32,
    /// Royalty recipient puzzle hash (defaults to `p2_puzzle_hash` if you mirror chia-sdk-driver).
    pub royalty_puzzle_hash: Bytes32,
    /// Royalty in basis points (e.g. 300 = 3%).
    pub royalty_basis_points: u16,
    /// Optional creator-DID attribution.
    pub did: Option<DidAttribution>,
}

/// The result of a mint: the coin spends to sign + a summary of the minted NFT for chaining/storage.
#[derive(Clone, Debug)]
pub struct NftMintResponse {
    /// Coin spends to be signed and broadcast.
    pub coin_spends: Vec<CoinSpend>,
    /// The minted NFT's launcher id (its permanent identifier).
    pub launcher_id: Bytes32,
    /// The minted NFT coin (the child after the eve spend).
    pub nft_coin: Coin,
}

/// Build the spend bundle that mints a single NFT whose media lives in a DIG capsule.
///
/// Spends the minter's `selected_coins` (first is the lead coin; the rest assert concurrent spend),
/// creates a singleton launcher from the lead coin, and mints the NFT via [`Launcher::mint_nft`]
/// with the supplied metadata, royalty, recipient, and optional DID attribution. Any value above
/// `fee + 1` mojo is returned to the minter as change. Returns the coin spends + the NFT summary.
///
/// The caller has already (a) written the media + CHIP-0007 metadata into a capsule via `digstore`
/// and (b) computed `data_hash`/`metadata_hash`/`license_hash` from the real bytes via
/// [`crate::metadata`]. This builder trusts those hashes; validating URI↔hash agreement is
/// [`crate::metadata::validate_uri_hash`]'s job before calling here.
///
/// # Errors
/// [`WalletError::Parse`] if `selected_coins` is empty; [`WalletError::Driver`] for any underlying
/// spend-construction failure.
pub fn mint_nft(
    minter_synthetic_key: PublicKey,
    selected_coins: Vec<Coin>,
    params: NftMintParams,
    fee: u64,
) -> Result<NftMintResponse, WalletError> {
    if selected_coins.is_empty() {
        return Err(WalletError::Parse("selected_coins is empty".to_string()));
    }

    let minter_puzzle_hash: Bytes32 = StandardArgs::curry_tree_hash(minter_synthetic_key).into();
    let total_amount_from_coins: u64 = selected_coins.iter().map(|c| c.amount).sum();
    let reserved = fee + 1; // 1 mojo funds the singleton launcher.

    let mut ctx = SpendContext::new();
    let p2 = StandardLayer::new(minter_synthetic_key);

    let lead_coin = selected_coins[0];
    let lead_coin_name = lead_coin.coin_id();

    for coin in selected_coins.into_iter().skip(1) {
        p2.spend(
            &mut ctx,
            coin,
            Conditions::new().assert_concurrent_spend(lead_coin_name),
        )?;
    }

    // Allocate the on-chain metadata and assemble the mint. Start from the canonical NftMint::new
    // (sets the default metadata-updater puzzle hash) then override royalty recipient + attribution.
    let metadata_ptr = ctx.alloc_hashed(&params.metadata.to_chain())?;
    let transfer_condition = params.did.as_ref().map(|did| {
        TransferNft::new(
            Some(did.launcher_id),
            Vec::new(),
            Some(did.inner_puzzle_hash),
        )
    });
    let mut nft_mint = NftMint::new(
        metadata_ptr,
        params.p2_puzzle_hash,
        params.royalty_basis_points,
        transfer_condition,
    );
    nft_mint.royalty_puzzle_hash = params.royalty_puzzle_hash;

    let launcher = Launcher::new(lead_coin_name, 1);
    let (mint_conditions, nft) = launcher.mint_nft(&mut ctx, &nft_mint)?;

    // The lead coin funds the launcher (1 mojo), carries the mint conditions, reserves the fee, and
    // returns change to the minter.
    let mut lead_conditions = mint_conditions;
    if fee > 0 {
        lead_conditions = lead_conditions.reserve_fee(fee);
    }
    if total_amount_from_coins > reserved {
        let hint = ctx.hint(minter_puzzle_hash)?;
        lead_conditions = lead_conditions.create_coin(
            minter_puzzle_hash,
            total_amount_from_coins - reserved,
            hint,
        );
    }
    p2.spend(&mut ctx, lead_coin, lead_conditions)?;

    Ok(NftMintResponse {
        coin_spends: ctx.take(),
        launcher_id: nft.info.launcher_id,
        nft_coin: nft.coin,
    })
}
