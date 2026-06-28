//! # chip35-dl-coin
//!
//! Isolated CHIP-0035 Chia DataLayer **store coin** driver. Provides the
//! offline spend builders to mint, update, and burn (melt) DataLayer stores,
//! plus keyless spend-bundle serialization helpers. No networking, no signing,
//! no key derivation — those are the caller's responsibility.

mod cat;
mod collection;
mod deploy_token;
mod did;
mod error;
mod metadata;
mod nft;
mod offer;
mod store;
mod types;

// Re-export the chia primitives consumers/bindings need.
pub use chia_bls::{master_to_wallet_unhardened, PublicKey, SecretKey, Signature};
pub use chia_protocol::{Bytes, Bytes32, Coin, CoinSpend, Program, SpendBundle};
pub use chia_puzzle_types::{EveProof, LineageProof, Proof};
pub use chia_sdk_driver::{
    DataStore, DataStoreInfo, DataStoreMetadata, DelegatedPuzzle, Did, DidInfo, HashedPtr,
};

pub use error::{Error, WalletError};
pub use store::{
    add_fee, datastore_from_spend, digstore_owner_hint, hex_spend_bundle_to_coin_spends,
    melt_store, mint_store, oracle_spend, spend_bundle_to_hex, update_store_metadata,
    update_store_ownership, DataStoreInnerSpend, DATASTORE_LAUNCHER_HINT,
    DIGSTORE_OWNER_HINT_DOMAIN,
};
pub use types::SuccessResponse;

// Asset toolkit (roadmap #33/#34/#35/#36).
pub use cat::{issue_cat, IssueCatResponse};
pub use collection::{
    build_bulk_mint, generate_item_metadata, synthetic_puzzle_hash, BulkMintResponse, Collection,
    ManifestItem, ManifestMedia,
};
pub use did::{create_did, CreateDidResponse};
pub use metadata::{
    sha256, validate_uri_hash, Attribute, Chip0007Metadata, CollectionRef, MetadataError,
    CHIP0007_FORMAT,
};
pub use nft::{mint_nft, DidAttribution, NftMediaMetadata, NftMintParams, NftMintResponse};
pub use offer::{decode_offer, encode_offer};

// Deploy-token delegation (roadmap #17) — SCAFFOLD pending security review (not wasm-exposed).
pub use deploy_token::{
    build_deploy_token, deploy_token_advance_root, DeployTokenTerms, SCAFFOLD_PENDING_REVIEW,
};
