//! # chip35-dl-coin
//!
//! Isolated CHIP-0035 Chia DataLayer **store coin** driver. Provides the
//! offline spend builders to mint, update, and burn (melt) DataLayer stores,
//! plus keyless spend-bundle serialization helpers. No networking, no signing,
//! no key derivation — those are the caller's responsibility.

mod cat;
mod collection;
mod did;
mod dig;
mod error;
mod gating;
mod metadata;
mod nft;
mod offer;
mod payment;
mod store;
mod subscription;
mod types;

// Re-export the chia primitives consumers/bindings need.
pub use chia_bls::{master_to_wallet_unhardened, PublicKey, SecretKey, Signature};
pub use chia_protocol::{Bytes, Bytes32, Coin, CoinSpend, Program, SpendBundle};
pub use chia_puzzle_types::{EveProof, LineageProof, Proof};
pub use chia_sdk_driver::{
    DataStore, DataStoreInfo, DataStoreMetadata, DelegatedPuzzle, Did, DidInfo, HashedPtr,
};

pub use error::{Error, WalletError};

// DIG-CAT per-capsule store payment (the canonical store-payment builder). Minting a store is FREE
// of $DIG; a capsule (commit / root-advance) pays the dynamic, USD-pegged per-capsule price in $DIG
// to the treasury via [`build_dig_store_payment`], concatenated atomically with the root-advance.
pub use dig::{
    build_dig_store_payment, dig_treasury_payment_coin, DIG_ASSET_ID,
    DIG_TREASURY_INNER_PUZZLE_HASH,
};

pub use store::{
    add_fee, admin_delegated_puzzle_from_key, datastore_from_spend, digstore_owner_hint,
    hex_spend_bundle_to_coin_spends, melt_store, mint_store, oracle_delegated_puzzle, oracle_spend,
    spend_bundle_to_hex, update_store_metadata, update_store_ownership,
    writer_delegated_puzzle_from_key, DataStoreInnerSpend, DATASTORE_LAUNCHER_HINT,
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
pub use nft::{
    mint_nft, mint_nft_with_did, DidAttribution, NftMediaMetadata, NftMintParams, NftMintResponse,
};
pub use offer::{decode_offer, encode_offer};

// In-dapp monetization (roadmap #46): payment, paywall (pay-to-unlock), NFT-gating, subscription
// scaffold. The dapp deployed on DIG EARNs — these are the inbound-economic primitives.
pub use gating::{
    prove_collection_membership, prove_nft_ownership, read_nft_ownership, GatingError,
    NftOwnershipProof,
};
pub use payment::{
    build_cat_payment, build_xch_payment, payment_nonce, verify_payment_receipt, ObservedPayment,
    PaymentAsset, PaymentReceipt, PaymentResponse, PaywallError,
};
pub use subscription::{
    build_subscription_authorization, build_subscription_claim, SubscriptionTerms,
};

// Re-export the Cat primitive (consumed by `build_cat_payment`'s caller to construct the buyer's CAT
// coins, and by the wasm boundary).
pub use chia_sdk_driver::{Cat, CatInfo};

// Deploy-token delegation (roadmap #17): a deploy token is a **revocable writer delegate**, not a
// bespoke puzzle. The prior hand-rolled scaffold (`deploy_token.rs`) is superseded — there is no
// special deploy-token type. To issue one, the owner adds [`writer_delegated_puzzle_from_key`] for
// the CI deploy key to the store's delegated-puzzle set via [`update_store_ownership`]; the deploy
// key then advances the root with [`update_store_metadata`] under [`DataStoreInnerSpend::Writer`]
// (a root advance IS a metadata update — no owner seed required). To revoke, the owner (or an admin
// delegate) replaces the delegated-puzzle set, dropping that writer. An on-chain DIG spend-cap that
// would further bound a deploy token is the only non-native extra and is future work.
