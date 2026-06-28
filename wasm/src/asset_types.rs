//! Serde boundary structs for the asset toolkit (NFT / collection / DID / CAT / metadata).
//!
//! Mirrors the conventions in [`crate::types`]: camelCase JS fields, 32-byte hashes as
//! `Uint8Array` (`serde_bytes`), u64 as BigInt. These map the JS shapes the asset SDK + CLI pass to
//! the native chip35-dl-coin asset builders.

use chip35_dl_coin::{
    Attribute as RustAttribute, Bytes32, Chip0007Metadata as RustChip0007Metadata,
    Collection as RustCollection, CollectionRef as RustCollectionRef,
    DidAttribution as RustDidAttribution, ManifestItem as RustManifestItem,
    ManifestMedia as RustManifestMedia, NftMediaMetadata as RustNftMediaMetadata,
    NftMintParams as RustNftMintParams,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

use crate::types::{bytes32, serde_bytes_opt};

/// JS shape of an NFT's on-chain media metadata (dig:// + https fallback URIs + computed hashes).
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NftMediaMetadata {
    #[serde(default)]
    pub data_uris: Vec<String>,
    #[serde(default, with = "serde_bytes_opt")]
    pub data_hash: Option<Vec<u8>>,
    #[serde(default)]
    pub metadata_uris: Vec<String>,
    #[serde(default, with = "serde_bytes_opt")]
    pub metadata_hash: Option<Vec<u8>>,
    #[serde(default)]
    pub license_uris: Vec<String>,
    #[serde(default, with = "serde_bytes_opt")]
    pub license_hash: Option<Vec<u8>>,
    #[serde(default)]
    pub edition_number: u64,
    #[serde(default)]
    pub edition_total: u64,
}

fn opt_bytes32(v: &Option<Vec<u8>>) -> Result<Option<Bytes32>, JsValue> {
    match v {
        Some(b) => Ok(Some(bytes32(b)?)),
        None => Ok(None),
    }
}

impl NftMediaMetadata {
    pub fn to_native(&self) -> Result<RustNftMediaMetadata, JsValue> {
        Ok(RustNftMediaMetadata {
            data_uris: self.data_uris.clone(),
            data_hash: opt_bytes32(&self.data_hash)?,
            metadata_uris: self.metadata_uris.clone(),
            metadata_hash: opt_bytes32(&self.metadata_hash)?,
            license_uris: self.license_uris.clone(),
            license_hash: opt_bytes32(&self.license_hash)?,
            edition_number: self.edition_number,
            edition_total: self.edition_total,
        })
    }
}

/// JS shape of a DID attribution (creator identity to attribute a mint to).
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidAttribution {
    #[serde(with = "serde_bytes")]
    pub launcher_id: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub inner_puzzle_hash: Vec<u8>,
}

impl DidAttribution {
    pub fn to_native(&self) -> Result<RustDidAttribution, JsValue> {
        Ok(RustDidAttribution {
            launcher_id: bytes32(&self.launcher_id)?,
            inner_puzzle_hash: bytes32(&self.inner_puzzle_hash)?,
        })
    }
}

/// JS shape of the parameters to mint a single NFT.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NftMintParams {
    pub metadata: NftMediaMetadata,
    #[serde(with = "serde_bytes")]
    pub p2_puzzle_hash: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub royalty_puzzle_hash: Vec<u8>,
    pub royalty_basis_points: u16,
    #[serde(default)]
    pub did: Option<DidAttribution>,
}

impl NftMintParams {
    pub fn to_native(&self) -> Result<RustNftMintParams, JsValue> {
        Ok(RustNftMintParams {
            metadata: self.metadata.to_native()?,
            p2_puzzle_hash: bytes32(&self.p2_puzzle_hash)?,
            royalty_puzzle_hash: bytes32(&self.royalty_puzzle_hash)?,
            royalty_basis_points: self.royalty_basis_points,
            did: match &self.did {
                Some(d) => Some(d.to_native()?),
                None => None,
            },
        })
    }
}

/// JS shape of a CHIP-0007 attribute (trait).
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attribute {
    pub trait_type: String,
    pub value: String,
}

impl Attribute {
    pub fn to_native(&self) -> RustAttribute {
        RustAttribute {
            trait_type: self.trait_type.clone(),
            value: self.value.clone(),
        }
    }
    pub fn from_native(a: &RustAttribute) -> Self {
        Attribute {
            trait_type: a.trait_type.clone(),
            value: a.value.clone(),
        }
    }
}

/// JS shape of a CHIP-0007 collection reference block (as embedded in item metadata).
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionRef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub attributes: Vec<Attribute>,
}

impl CollectionRef {
    pub fn from_native(c: &RustCollectionRef) -> Self {
        CollectionRef {
            id: c.id.clone(),
            name: c.name.clone(),
            attributes: c.attributes.iter().map(Attribute::from_native).collect(),
        }
    }
}

/// JS shape of a CHIP-0007 metadata document (the off-chain JSON).
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chip0007Metadata {
    #[serde(default = "default_format")]
    pub format: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub sensitive_content: bool,
    #[serde(default)]
    pub collection: Option<CollectionRef>,
    #[serde(default)]
    pub attributes: Vec<Attribute>,
    #[serde(default)]
    pub series_number: Option<u64>,
    #[serde(default)]
    pub series_total: Option<u64>,
    #[serde(default)]
    pub minting_tool: Option<String>,
}

fn default_format() -> String {
    chip35_dl_coin::CHIP0007_FORMAT.to_string()
}

impl Chip0007Metadata {
    pub fn to_native(&self) -> RustChip0007Metadata {
        RustChip0007Metadata {
            format: self.format.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            sensitive_content: self.sensitive_content,
            collection: self.collection.as_ref().map(|c| RustCollectionRef {
                id: c.id.clone(),
                name: c.name.clone(),
                attributes: c.attributes.iter().map(Attribute::to_native).collect(),
            }),
            attributes: self.attributes.iter().map(Attribute::to_native).collect(),
            series_number: self.series_number,
            series_total: self.series_total,
            minting_tool: self.minting_tool.clone(),
        }
    }
    pub fn from_native(m: &RustChip0007Metadata) -> Self {
        Chip0007Metadata {
            format: m.format.clone(),
            name: m.name.clone(),
            description: m.description.clone(),
            sensitive_content: m.sensitive_content,
            collection: m.collection.as_ref().map(CollectionRef::from_native),
            attributes: m.attributes.iter().map(Attribute::from_native).collect(),
            series_number: m.series_number,
            series_total: m.series_total,
            minting_tool: m.minting_tool.clone(),
        }
    }
}

/// JS shape of a collection definition.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub attributes: Vec<Attribute>,
    #[serde(with = "serde_bytes")]
    pub royalty_puzzle_hash: Vec<u8>,
    pub royalty_basis_points: u16,
}

impl Collection {
    pub fn to_native(&self) -> Result<RustCollection, JsValue> {
        Ok(RustCollection {
            id: self.id.clone(),
            name: self.name.clone(),
            attributes: self.attributes.iter().map(Attribute::to_native).collect(),
            royalty_puzzle_hash: bytes32(&self.royalty_puzzle_hash)?,
            royalty_basis_points: self.royalty_basis_points,
        })
    }
}

/// JS shape of one item's on-chain media in a parsed traits manifest.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestMedia {
    #[serde(default)]
    pub data_uris: Vec<String>,
    #[serde(default, with = "serde_bytes_opt")]
    pub data_hash: Option<Vec<u8>>,
    #[serde(default)]
    pub metadata_uris: Vec<String>,
    #[serde(default, with = "serde_bytes_opt")]
    pub metadata_hash: Option<Vec<u8>>,
    #[serde(default)]
    pub license_uris: Vec<String>,
    #[serde(default, with = "serde_bytes_opt")]
    pub license_hash: Option<Vec<u8>>,
}

impl ManifestMedia {
    pub fn to_native(&self) -> Result<RustManifestMedia, JsValue> {
        Ok(RustManifestMedia {
            data_uris: self.data_uris.clone(),
            data_hash: opt_bytes32(&self.data_hash)?,
            metadata_uris: self.metadata_uris.clone(),
            metadata_hash: opt_bytes32(&self.metadata_hash)?,
            license_uris: self.license_uris.clone(),
            license_hash: opt_bytes32(&self.license_hash)?,
        })
    }
}

/// JS shape of one item in a parsed traits manifest.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestItem {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub attributes: Vec<Attribute>,
    pub media: ManifestMedia,
}

impl ManifestItem {
    pub fn to_native(&self) -> Result<RustManifestItem, JsValue> {
        Ok(RustManifestItem {
            name: self.name.clone(),
            description: self.description.clone(),
            attributes: self.attributes.iter().map(Attribute::to_native).collect(),
            media: self.media.to_native()?,
        })
    }
}
