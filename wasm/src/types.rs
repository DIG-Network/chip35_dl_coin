//! Serde boundary structs and conversions to/from native chip35-dl-coin types.

use chip35_dl_coin::{Bytes32, Program, PublicKey, Signature};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use wasm_bindgen::JsValue;

/// Serialize a Rust value into a JS value, encoding u64/i64 as BigInt
/// (mojo amounts exceed 2^53, so the default lossy f64 path is unacceptable).
pub fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    let ser = serde_wasm_bindgen::Serializer::new().serialize_large_number_types_as_bigints(true);
    value
        .serialize(&ser)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}

/// Deserialize a JS value into a Rust value. Accepts JS number and BigInt for integers.
pub fn from_js<T: DeserializeOwned>(value: JsValue) -> Result<T, JsValue> {
    serde_wasm_bindgen::from_value(value).map_err(|e| JsValue::from_str(&e.to_string()))
}

pub fn bytes32(buf: &[u8]) -> Result<Bytes32, JsValue> {
    Bytes32::try_from(buf.to_vec()).map_err(|_| JsValue::from_str("expected 32-byte value"))
}

pub fn public_key(buf: &[u8]) -> Result<PublicKey, JsValue> {
    let arr =
        <[u8; 48]>::try_from(buf).map_err(|_| JsValue::from_str("expected 48-byte public key"))?;
    PublicKey::from_bytes(&arr).map_err(|_| JsValue::from_str("invalid public key"))
}

pub fn signature(buf: &[u8]) -> Result<Signature, JsValue> {
    let arr =
        <[u8; 96]>::try_from(buf).map_err(|_| JsValue::from_str("expected 96-byte signature"))?;
    Signature::from_bytes(&arr).map_err(|_| JsValue::from_str("invalid signature"))
}

use chip35_dl_coin::{
    Coin as RustCoin, CoinSpend as RustCoinSpend, EveProof as RustEveProof,
    LineageProof as RustLineageProof, Proof as RustProof,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Coin {
    #[serde(with = "serde_bytes")]
    pub parent_coin_info: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub puzzle_hash: Vec<u8>,
    pub amount: u64,
}

impl Coin {
    pub fn to_native(&self) -> Result<RustCoin, JsValue> {
        Ok(RustCoin {
            parent_coin_info: bytes32(&self.parent_coin_info)?,
            puzzle_hash: bytes32(&self.puzzle_hash)?,
            amount: self.amount,
        })
    }

    pub fn from_native(c: &RustCoin) -> Self {
        Coin {
            parent_coin_info: c.parent_coin_info.to_vec(),
            puzzle_hash: c.puzzle_hash.to_vec(),
            amount: c.amount,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoinSpend {
    pub coin: Coin,
    #[serde(with = "serde_bytes")]
    pub puzzle_reveal: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub solution: Vec<u8>,
}

impl CoinSpend {
    pub fn to_native(&self) -> Result<RustCoinSpend, JsValue> {
        Ok(RustCoinSpend {
            coin: self.coin.to_native()?,
            puzzle_reveal: Program::from(self.puzzle_reveal.clone()),
            solution: Program::from(self.solution.clone()),
        })
    }

    pub fn from_native(cs: &RustCoinSpend) -> Self {
        CoinSpend {
            coin: Coin::from_native(&cs.coin),
            puzzle_reveal: cs.puzzle_reveal.to_vec(),
            solution: cs.solution.to_vec(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineageProof {
    #[serde(with = "serde_bytes")]
    pub parent_parent_coin_info: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub parent_inner_puzzle_hash: Vec<u8>,
    pub parent_amount: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EveProof {
    #[serde(with = "serde_bytes")]
    pub parent_parent_coin_info: Vec<u8>,
    pub parent_amount: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Proof {
    pub lineage_proof: Option<LineageProof>,
    pub eve_proof: Option<EveProof>,
}

impl Proof {
    pub fn to_native(&self) -> Result<RustProof, JsValue> {
        if let Some(lp) = &self.lineage_proof {
            Ok(RustProof::Lineage(RustLineageProof {
                parent_parent_coin_info: bytes32(&lp.parent_parent_coin_info)?,
                parent_inner_puzzle_hash: bytes32(&lp.parent_inner_puzzle_hash)?,
                parent_amount: lp.parent_amount,
            }))
        } else if let Some(ep) = &self.eve_proof {
            Ok(RustProof::Eve(RustEveProof {
                parent_parent_coin_info: bytes32(&ep.parent_parent_coin_info)?,
                parent_amount: ep.parent_amount,
            }))
        } else {
            Err(JsValue::from_str("missing proof"))
        }
    }

    pub fn from_native(p: &RustProof) -> Self {
        match p {
            RustProof::Lineage(lp) => Proof {
                lineage_proof: Some(LineageProof {
                    parent_parent_coin_info: lp.parent_parent_coin_info.to_vec(),
                    parent_inner_puzzle_hash: lp.parent_inner_puzzle_hash.to_vec(),
                    parent_amount: lp.parent_amount,
                }),
                eve_proof: None,
            },
            RustProof::Eve(ep) => Proof {
                lineage_proof: None,
                eve_proof: Some(EveProof {
                    parent_parent_coin_info: ep.parent_parent_coin_info.to_vec(),
                    parent_amount: ep.parent_amount,
                }),
            },
        }
    }
}

use chip35_dl_coin::{
    DataStore as RustDataStore, DataStoreInfo as RustDataStoreInfo,
    DataStoreMetadata as RustDataStoreMetadata, DelegatedPuzzle as RustDelegatedPuzzle,
    SuccessResponse as RustSuccessResponse,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataStoreMetadata {
    #[serde(with = "serde_bytes")]
    pub root_hash: Vec<u8>,
    pub label: Option<String>,
    pub description: Option<String>,
    pub bytes: Option<u64>,
    #[serde(default, with = "serde_bytes_opt")]
    pub size_proof: Option<Vec<u8>>,
}

impl DataStoreMetadata {
    pub fn to_native(&self) -> Result<RustDataStoreMetadata, JsValue> {
        Ok(RustDataStoreMetadata {
            root_hash: bytes32(&self.root_hash)?,
            label: self.label.clone(),
            description: self.description.clone(),
            bytes: self.bytes,
            size_proof: match &self.size_proof {
                Some(sp) => Some(bytes32(sp)?.to_string()),
                None => None,
            },
        })
    }

    pub fn from_native(m: &RustDataStoreMetadata) -> Result<Self, JsValue> {
        Ok(DataStoreMetadata {
            root_hash: m.root_hash.to_vec(),
            label: m.label.clone(),
            description: m.description.clone(),
            bytes: m.bytes,
            size_proof: match &m.size_proof {
                Some(s) => Some(
                    hex::decode(s.trim_start_matches("0x"))
                        .map_err(|_| JsValue::from_str("invalid size_proof hex"))?,
                ),
                None => None,
            },
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegatedPuzzle {
    #[serde(default, with = "serde_bytes_opt")]
    pub admin_inner_puzzle_hash: Option<Vec<u8>>,
    #[serde(default, with = "serde_bytes_opt")]
    pub writer_inner_puzzle_hash: Option<Vec<u8>>,
    #[serde(default, with = "serde_bytes_opt")]
    pub oracle_payment_puzzle_hash: Option<Vec<u8>>,
    pub oracle_fee: Option<u64>,
}

mod serde_bytes_opt {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(b) => serde_bytes::serialize(b, s),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
        let opt: Option<serde_bytes::ByteBuf> = Option::deserialize(d)?;
        Ok(opt.map(|b| b.into_vec()))
    }
}

impl DelegatedPuzzle {
    pub fn to_native(&self) -> Result<RustDelegatedPuzzle, JsValue> {
        if let Some(h) = &self.admin_inner_puzzle_hash {
            Ok(RustDelegatedPuzzle::Admin(bytes32(h)?.into()))
        } else if let Some(h) = &self.writer_inner_puzzle_hash {
            Ok(RustDelegatedPuzzle::Writer(bytes32(h)?.into()))
        } else if let (Some(h), Some(fee)) = (&self.oracle_payment_puzzle_hash, self.oracle_fee) {
            Ok(RustDelegatedPuzzle::Oracle(bytes32(h)?, fee))
        } else {
            Err(JsValue::from_str("missing delegated puzzle info"))
        }
    }

    pub fn from_native(d: &RustDelegatedPuzzle) -> Result<Self, JsValue> {
        Ok(match d {
            RustDelegatedPuzzle::Admin(th) => {
                let b32: Bytes32 = (*th).into();
                DelegatedPuzzle {
                    admin_inner_puzzle_hash: Some(b32.to_vec()),
                    writer_inner_puzzle_hash: None,
                    oracle_payment_puzzle_hash: None,
                    oracle_fee: None,
                }
            }
            RustDelegatedPuzzle::Writer(th) => {
                let b32: Bytes32 = (*th).into();
                DelegatedPuzzle {
                    admin_inner_puzzle_hash: None,
                    writer_inner_puzzle_hash: Some(b32.to_vec()),
                    oracle_payment_puzzle_hash: None,
                    oracle_fee: None,
                }
            }
            RustDelegatedPuzzle::Oracle(h, fee) => DelegatedPuzzle {
                admin_inner_puzzle_hash: None,
                writer_inner_puzzle_hash: None,
                oracle_payment_puzzle_hash: Some(h.to_vec()),
                oracle_fee: Some(*fee),
            },
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataStore {
    pub coin: Coin,
    #[serde(with = "serde_bytes")]
    pub launcher_id: Vec<u8>,
    pub proof: Proof,
    pub metadata: DataStoreMetadata,
    #[serde(with = "serde_bytes")]
    pub owner_puzzle_hash: Vec<u8>,
    pub delegated_puzzles: Vec<DelegatedPuzzle>,
}

impl DataStore {
    pub fn to_native(&self) -> Result<RustDataStore, JsValue> {
        Ok(RustDataStore {
            coin: self.coin.to_native()?,
            proof: self.proof.to_native()?,
            info: RustDataStoreInfo {
                launcher_id: bytes32(&self.launcher_id)?,
                metadata: self.metadata.to_native()?,
                owner_puzzle_hash: bytes32(&self.owner_puzzle_hash)?,
                delegated_puzzles: self
                    .delegated_puzzles
                    .iter()
                    .map(DelegatedPuzzle::to_native)
                    .collect::<Result<Vec<_>, _>>()?,
            },
        })
    }

    pub fn from_native(s: &RustDataStore) -> Result<Self, JsValue> {
        Ok(DataStore {
            coin: Coin::from_native(&s.coin),
            launcher_id: s.info.launcher_id.to_vec(),
            proof: Proof::from_native(&s.proof),
            metadata: DataStoreMetadata::from_native(&s.info.metadata)?,
            owner_puzzle_hash: s.info.owner_puzzle_hash.to_vec(),
            delegated_puzzles: s
                .info
                .delegated_puzzles
                .iter()
                .map(DelegatedPuzzle::from_native)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuccessResponse {
    pub coin_spends: Vec<CoinSpend>,
    pub new_store: DataStore,
}

impl SuccessResponse {
    pub fn from_native(r: &RustSuccessResponse) -> Result<Self, JsValue> {
        Ok(SuccessResponse {
            coin_spends: r.coin_spends.iter().map(CoinSpend::from_native).collect(),
            new_store: DataStore::from_native(&r.new_datastore)?,
        })
    }
}

pub fn coins_from_js(value: JsValue) -> Result<Vec<RustCoin>, JsValue> {
    let coins: Vec<Coin> = from_js(value)?;
    coins.iter().map(Coin::to_native).collect()
}

pub fn delegated_puzzles_from_js(value: JsValue) -> Result<Vec<RustDelegatedPuzzle>, JsValue> {
    let dps: Vec<DelegatedPuzzle> = from_js(value)?;
    dps.iter().map(DelegatedPuzzle::to_native).collect()
}

pub fn coin_spends_to_js(css: &[RustCoinSpend]) -> Result<JsValue, JsValue> {
    let out: Vec<CoinSpend> = css.iter().map(CoinSpend::from_native).collect();
    to_js(&out)
}
