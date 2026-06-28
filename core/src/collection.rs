//! Collection primitive + traits-manifest bulk mint (roadmap #34).
//!
//! Creators think in *collections*, not individual mints. This models a CHIP-0007 collection
//! (id/name/attributes/shared royalty + DID + license), generates per-item CHIP-0007 metadata from
//! a *parsed* traits manifest (JSON in — no file IO; the CLI/toolkit parses CSV/JSON into the
//! manifest), and bulk-mints all items in one bundle using intermediate launchers.

use chia_bls::PublicKey;
use chia_protocol::{Bytes32, CoinSpend};
use chia_puzzle_types::nft::NftMetadata;
use chia_puzzle_types::standard::StandardArgs;
use chia_sdk_driver::{
    Did, IntermediateLauncher, NftMint, SingletonInfo, SpendContext, StandardLayer,
};
use chia_sdk_types::{conditions::TransferNft, Conditions};
use serde::{Deserialize, Serialize};

use crate::error::WalletError;
use crate::metadata::{Attribute, Chip0007Metadata, CollectionRef};
use crate::nft::{DidAttribution, NftMediaMetadata};

/// A CHIP-0007 collection definition: the shared identity + economics across every item.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Collection {
    /// Stable collection id (the toolkit derives it from the creator DID + name, or supplies it).
    pub id: String,
    /// Human-readable collection name.
    pub name: String,
    /// Collection-level attributes (icon/banner/website/twitter/etc) as CHIP-0007 name/value pairs.
    #[serde(default)]
    pub attributes: Vec<Attribute>,
    /// Shared royalty recipient puzzle hash for every item.
    pub royalty_puzzle_hash: Bytes32,
    /// Shared royalty in basis points for every item.
    pub royalty_basis_points: u16,
}

impl Collection {
    /// The [`CollectionRef`] block embedded into each item's CHIP-0007 metadata.
    pub fn as_ref_block(&self) -> CollectionRef {
        CollectionRef {
            id: self.id.clone(),
            name: self.name.clone(),
            attributes: self.attributes.clone(),
        }
    }
}

/// One item in a parsed traits manifest. The toolkit produces this from a CSV/JSON manifest +
/// the per-item capsule hashes; this crate consumes the parsed form only (no file IO).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManifestItem {
    /// The item's name (e.g. `"DIG Punk #12"`).
    pub name: String,
    /// Optional per-item description.
    #[serde(default)]
    pub description: Option<String>,
    /// Per-item traits.
    #[serde(default)]
    pub attributes: Vec<Attribute>,
    /// On-chain media metadata + hashes for this item (dig:// + https fallback URIs).
    pub media: ManifestMedia,
}

/// The on-chain media fields for a manifest item (mirrors [`NftMediaMetadata`] in a serde-friendly,
/// hex-hash shape for the parsed manifest).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ManifestMedia {
    /// Primary media URIs (dig:// first, https fallback second by convention).
    #[serde(default)]
    pub data_uris: Vec<String>,
    /// `sha256(media_bytes)`.
    #[serde(default)]
    pub data_hash: Option<Bytes32>,
    /// CHIP-0007 metadata JSON URIs.
    #[serde(default)]
    pub metadata_uris: Vec<String>,
    /// `sha256(metadata_json_bytes)`.
    #[serde(default)]
    pub metadata_hash: Option<Bytes32>,
    /// License document URIs.
    #[serde(default)]
    pub license_uris: Vec<String>,
    /// `sha256(license_bytes)`.
    #[serde(default)]
    pub license_hash: Option<Bytes32>,
}

impl ManifestMedia {
    fn to_media(&self, edition_number: u64, edition_total: u64) -> NftMediaMetadata {
        NftMediaMetadata {
            data_uris: self.data_uris.clone(),
            data_hash: self.data_hash,
            metadata_uris: self.metadata_uris.clone(),
            metadata_hash: self.metadata_hash,
            license_uris: self.license_uris.clone(),
            license_hash: self.license_hash,
            edition_number,
            edition_total,
        }
    }
}

/// Generate the per-item CHIP-0007 metadata documents for a collection from a parsed manifest.
///
/// Each item gets the collection block, its own traits, and `series_number`/`series_total` filled in
/// (1-based). This is the off-chain JSON side; the on-chain hashes come from [`ManifestMedia`]. The
/// toolkit hashes each generated document and writes it into the item's capsule.
pub fn generate_item_metadata(
    collection: &Collection,
    items: &[ManifestItem],
) -> Vec<Chip0007Metadata> {
    let total = items.len() as u64;
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let mut md = Chip0007Metadata::new(item.name.clone());
            md.description = item.description.clone();
            md.attributes = item.attributes.clone();
            md.collection = Some(collection.as_ref_block());
            md.series_number = Some(i as u64 + 1);
            md.series_total = Some(total);
            md.minting_tool = Some("DIG".to_string());
            md
        })
        .collect()
}

/// The result of a bulk mint: the coin spends + the launcher id of every minted NFT (in order).
#[derive(Clone, Debug)]
pub struct BulkMintResponse {
    /// Coin spends to be signed and broadcast.
    pub coin_spends: Vec<CoinSpend>,
    /// The minted NFTs' launcher ids, in manifest order.
    pub launcher_ids: Vec<Bytes32>,
}

/// Build the spend bundle that bulk-mints every item in `items` into `collection`, attributed to
/// `did`, authorized by a single DID spend.
///
/// Uses the canonical chia-sdk-driver bulk-mint pattern: one [`IntermediateLauncher`] per item
/// (`mint_number`, `mint_total`) → [`Launcher::mint_nft`], with the collection's shared royalty and
/// the DID `TransferNft` attribution, then spends the DID once with all the mint conditions
/// extended together. The DID coin is supplied by the caller (created via [`crate::did::create_did`]
/// or fetched on-chain) and must be spendable by `minter_synthetic_key`.
///
/// `recipient_puzzle_hash` owns every minted NFT. Returns the coin spends + the launcher ids.
///
/// [`Launcher::mint_nft`]: chia_sdk_driver::Launcher::mint_nft
///
/// # Errors
/// [`WalletError::Parse`] if `items` is empty; [`WalletError::Driver`] on spend-construction failure.
#[allow(clippy::too_many_arguments)]
pub fn build_bulk_mint(
    minter_synthetic_key: PublicKey,
    did: Did,
    collection: &Collection,
    items: &[ManifestItem],
    recipient_puzzle_hash: Bytes32,
) -> Result<BulkMintResponse, WalletError> {
    if items.is_empty() {
        return Err(WalletError::Parse("items is empty".to_string()));
    }

    let mut ctx = SpendContext::new();
    let p2 = StandardLayer::new(minter_synthetic_key);

    let did_attr = DidAttribution {
        launcher_id: did.info.launcher_id,
        inner_puzzle_hash: did.info.inner_puzzle_hash().into(),
    };

    let total = items.len();
    let mut all_mint_conditions = Conditions::new();
    let mut launcher_ids = Vec::with_capacity(total);

    for (i, item) in items.iter().enumerate() {
        let metadata_ptr =
            ctx.alloc_hashed(&item_to_chain_metadata(item, i as u64 + 1, total as u64))?;
        let transfer = TransferNft::new(
            Some(did_attr.launcher_id),
            Vec::new(),
            Some(did_attr.inner_puzzle_hash),
        );
        let mut nft_mint = NftMint::new(
            metadata_ptr,
            recipient_puzzle_hash,
            collection.royalty_basis_points,
            Some(transfer),
        );
        nft_mint.royalty_puzzle_hash = collection.royalty_puzzle_hash;

        let (mint_conditions, nft) = IntermediateLauncher::new(did.coin.coin_id(), i, total)
            .create(&mut ctx)?
            .mint_nft(&mut ctx, &nft_mint)?;
        all_mint_conditions = all_mint_conditions.extend(mint_conditions);
        launcher_ids.push(nft.info.launcher_id);
    }

    // Spend the DID once, authorizing all mints. `did.update` re-creates the DID under its own
    // standard inner puzzle, emitting the collected mint conditions; the recreated DID is not needed
    // here (the caller re-fetches it on-chain if it wants to chain further mints).
    let _recreated_did = did.update(&mut ctx, &p2, all_mint_conditions)?;

    Ok(BulkMintResponse {
        coin_spends: ctx.take(),
        launcher_ids,
    })
}

/// Convert a manifest item into the on-chain [`NftMetadata`] CLVM struct for one bulk-mint slot.
fn item_to_chain_metadata(
    item: &ManifestItem,
    edition_number: u64,
    edition_total: u64,
) -> NftMetadata {
    let media = item.media.to_media(edition_number, edition_total);
    NftMetadata {
        edition_number: media.edition_number,
        edition_total: media.edition_total,
        data_uris: media.data_uris,
        data_hash: media.data_hash,
        metadata_uris: media.metadata_uris,
        metadata_hash: media.metadata_hash,
        license_uris: media.license_uris,
        license_hash: media.license_hash,
    }
}

/// Compute the standard puzzle hash for a synthetic key — the convenience used to default the
/// bulk-mint recipient to the minter's own wallet.
pub fn synthetic_puzzle_hash(synthetic_key: PublicKey) -> Bytes32 {
    StandardArgs::curry_tree_hash(synthetic_key).into()
}
