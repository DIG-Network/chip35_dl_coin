//! Serde boundary structs for the trustless lazy-mint wasm exports (roadmap #40).
//!
//! Mirrors the conventions in [`crate::types`]: camelCase JS fields, 32-byte hashes as `Uint8Array`
//! (`serde_bytes`), u64 as BigInt. The `descriptor` a commit returns is an OPAQUE JSON string the
//! caller persists and passes straight back to a claim — the claimer never hand-builds it, so the
//! whole committed tree (collection / items / policy / commitment coins / launcher ids) round-trips
//! byte-exactly without re-deriving any nested shape at the boundary.

use chip35_dl_coin::{
    Bytes32, LazyMintItem as RustLazyMintItem, LazyMintPolicy as RustLazyMintPolicy,
    LazyMintTreeDescriptor as RustDescriptor, MerkleMembershipProof as RustMerkleProof,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

use crate::asset_types::NftMediaMetadata;
use crate::monetization_types::PaymentAsset;
use crate::types::{bytes32, js_err};

/// JS shape of the recipient/payment policy for a lazy mint:
/// `{ directMint: true }` (free, simulator-validated) or
/// `{ paymentGated: { price, asset, payee } }` (payment ENFORCEMENT deferred — see DESIGN.md #40).
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LazyMintPolicy {
    #[serde(default)]
    pub direct_mint: bool,
    #[serde(default)]
    pub payment_gated: Option<PaymentGated>,
}

/// The `paymentGated` arm payload.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentGated {
    pub price: u64,
    pub asset: PaymentAsset,
    #[serde(with = "serde_bytes")]
    pub payee: Vec<u8>,
}

impl LazyMintPolicy {
    pub fn to_native(&self) -> Result<RustLazyMintPolicy, JsValue> {
        match (&self.payment_gated, self.direct_mint) {
            (Some(pg), _) => Ok(RustLazyMintPolicy::PaymentGated {
                price: pg.price,
                asset: pg.asset.to_native()?,
                payee: bytes32(&pg.payee)?,
            }),
            (None, true) => Ok(RustLazyMintPolicy::DirectMint),
            (None, false) => Err(js_err(
                "INVALID_ARGUMENT",
                "policy must set either directMint:true or paymentGated",
            )),
        }
    }
}

/// JS shape of one precommitted item: its on-chain media metadata + royalty.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LazyMintItem {
    pub metadata: NftMediaMetadata,
    pub royalty_basis_points: u16,
}

impl LazyMintItem {
    pub fn to_native(&self) -> Result<RustLazyMintItem, JsValue> {
        Ok(RustLazyMintItem {
            metadata: self.metadata.to_native()?,
            royalty_basis_points: self.royalty_basis_points,
        })
    }
}

/// JS shape of a merkle membership proof for an allowlist-gated claim (on-chain ENFORCEMENT deferred).
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MerkleMembershipProof {
    pub path: u32,
    /// Sibling hashes along the path (`Uint8Array[]`).
    pub proof: Vec<serde_bytes::ByteBuf>,
}

impl MerkleMembershipProof {
    pub fn to_native(&self) -> Result<RustMerkleProof, JsValue> {
        let proof = self
            .proof
            .iter()
            .map(|b| bytes32(b))
            .collect::<Result<Vec<Bytes32>, _>>()?;
        Ok(RustMerkleProof {
            path: self.path,
            proof,
        })
    }
}

/// Serialize a native [`RustDescriptor`] into the opaque JSON-string handle returned to JS.
pub fn descriptor_to_json(d: &RustDescriptor) -> Result<String, JsValue> {
    serde_json::to_string(d).map_err(|e| js_err("SERDE_ERROR", e.to_string()))
}

/// Parse the opaque JSON-string handle back into a native [`RustDescriptor`].
pub fn descriptor_from_json(s: &str) -> Result<RustDescriptor, JsValue> {
    serde_json::from_str(s).map_err(|e| js_err("SERDE_ERROR", e.to_string()))
}
