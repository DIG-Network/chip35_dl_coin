//! Serde boundary structs for the in-dapp monetization wasm exports (roadmap #46): the buyer's CAT
//! coin shape (for CAT payments), the payment receipt + observed-payment shapes (for the paywall),
//! and the NFT-gating proof shape. Mirrors the conventions in [`crate::types`]: camelCase JS fields,
//! 32-byte hashes as `Uint8Array` (`serde_bytes`), u64 as BigInt.

use chip35_dl_coin::{
    Bytes32, Cat as RustCat, CatInfo as RustCatInfo, ObservedPayment as RustObservedPayment,
    PaymentAsset as RustPaymentAsset, PaymentReceipt as RustPaymentReceipt,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

use crate::types::{bytes32, js_err, serde_bytes_opt, Coin, LineageProof};

/// JS shape of a payment asset: `{ xch: true }` for XCH, or `{ assetId: Uint8Array }` for a CAT.
/// Exactly one is set. Represented as a struct (not an enum) so the JS object is ergonomic.
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PaymentAsset {
    #[serde(default)]
    pub xch: bool,
    #[serde(default, with = "serde_bytes_opt")]
    pub asset_id: Option<Vec<u8>>,
}

impl PaymentAsset {
    pub fn to_native(&self) -> Result<RustPaymentAsset, JsValue> {
        match (&self.asset_id, self.xch) {
            (Some(id), _) => Ok(RustPaymentAsset::Cat(bytes32(id)?)),
            (None, true) => Ok(RustPaymentAsset::Xch),
            (None, false) => Err(js_err(
                "INVALID_ARGUMENT",
                "payment asset must set either xch:true or assetId",
            )),
        }
    }

    pub fn from_native(a: &RustPaymentAsset) -> Self {
        match a {
            RustPaymentAsset::Xch => PaymentAsset {
                xch: true,
                asset_id: None,
            },
            RustPaymentAsset::Cat(id) => PaymentAsset {
                xch: false,
                asset_id: Some(id.to_vec()),
            },
        }
    }
}

/// JS shape of a CAT's on-chain info (asset id + optional revocation hidden puzzle + p2 puzzle hash).
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatInfo {
    #[serde(with = "serde_bytes")]
    pub asset_id: Vec<u8>,
    #[serde(default, with = "serde_bytes_opt")]
    pub hidden_puzzle_hash: Option<Vec<u8>>,
    #[serde(with = "serde_bytes")]
    pub p2_puzzle_hash: Vec<u8>,
}

/// JS shape of a buyer's CAT coin (as `chip0002_getAssetCoins` returns it): the coin, its optional
/// lineage proof, and its CAT info. Feeds [`build_cat_payment`](chip35_dl_coin::build_cat_payment).
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cat {
    pub coin: Coin,
    #[serde(default)]
    pub lineage_proof: Option<LineageProof>,
    pub info: CatInfo,
}

impl Cat {
    pub fn to_native(&self) -> Result<RustCat, JsValue> {
        let lineage = match &self.lineage_proof {
            Some(lp) => Some(chip35_dl_coin::LineageProof {
                parent_parent_coin_info: bytes32(&lp.parent_parent_coin_info)?,
                parent_inner_puzzle_hash: bytes32(&lp.parent_inner_puzzle_hash)?,
                parent_amount: lp.parent_amount,
            }),
            None => None,
        };
        let hidden = match &self.info.hidden_puzzle_hash {
            Some(h) => Some(bytes32(h)?),
            None => None,
        };
        Ok(RustCat::new(
            self.coin.to_native()?,
            lineage,
            RustCatInfo::new(
                bytes32(&self.info.asset_id)?,
                hidden,
                bytes32(&self.info.p2_puzzle_hash)?,
            ),
        ))
    }
}

/// JS shape of a [`PaymentReceipt`](chip35_dl_coin::PaymentReceipt) returned by the payment builders.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentReceipt {
    #[serde(with = "serde_bytes")]
    pub owner_puzzle_hash: Vec<u8>,
    pub amount: u64,
    pub asset: PaymentAsset,
    #[serde(with = "serde_bytes")]
    pub nonce: Vec<u8>,
    pub payment_coin: Coin,
}

impl PaymentReceipt {
    pub fn from_native(r: &RustPaymentReceipt) -> Self {
        PaymentReceipt {
            owner_puzzle_hash: r.owner_puzzle_hash.to_vec(),
            amount: r.amount,
            asset: PaymentAsset::from_native(&r.asset),
            nonce: r.nonce.to_vec(),
            payment_coin: Coin::from_native(&r.payment_coin),
        }
    }
}

/// JS shape of an [`ObservedPayment`](chip35_dl_coin::ObservedPayment) the dapp passes to the paywall
/// check after reading the owner's coin from the chain.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedPayment {
    #[serde(with = "serde_bytes")]
    pub paid_to_puzzle_hash: Vec<u8>,
    pub amount: u64,
    pub asset: PaymentAsset,
    #[serde(default, with = "serde_bytes_opt")]
    pub nonce: Option<Vec<u8>>,
}

impl ObservedPayment {
    pub fn to_native(&self) -> Result<RustObservedPayment, JsValue> {
        let nonce = match &self.nonce {
            Some(n) => Some(bytes32(n)?),
            None => None,
        };
        Ok(RustObservedPayment {
            paid_to_puzzle_hash: bytes32(&self.paid_to_puzzle_hash)?,
            amount: self.amount,
            asset: self.asset.to_native()?,
            nonce,
        })
    }
}

/// JS shape of an [`NftOwnershipProof`](chip35_dl_coin::NftOwnershipProof) returned by the gating
/// helpers (all read facts).
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NftOwnershipProof {
    #[serde(with = "serde_bytes")]
    pub launcher_id: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub owner_puzzle_hash: Vec<u8>,
    #[serde(default, with = "serde_bytes_opt")]
    pub attributed_did: Option<Vec<u8>>,
    #[serde(with = "serde_bytes")]
    pub nft_coin_id: Vec<u8>,
}

impl NftOwnershipProof {
    pub fn from_native(p: &chip35_dl_coin::NftOwnershipProof) -> Self {
        NftOwnershipProof {
            launcher_id: p.launcher_id.to_vec(),
            owner_puzzle_hash: p.owner_puzzle_hash.to_vec(),
            attributed_did: p.attributed_did.map(|d| d.to_vec()),
            nft_coin_id: p.nft_coin_id.to_vec(),
        }
    }
}

/// Helper: parse an optional 32-byte JS value into `Option<Bytes32>`.
pub fn opt_bytes32(v: Option<Vec<u8>>) -> Result<Option<Bytes32>, JsValue> {
    match v {
        Some(b) => Ok(Some(bytes32(&b)?)),
        None => Ok(None),
    }
}
