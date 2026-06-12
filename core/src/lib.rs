//! # chip35-dl-coin
//!
//! Isolated CHIP-0035 Chia DataLayer **store coin** driver. Provides the
//! offline spend builders to mint, update, and burn (melt) DataLayer stores,
//! plus keyless spend-bundle serialization helpers. No networking, no signing,
//! no key derivation — those are the caller's responsibility.

mod error;
mod store;
mod types;

// Re-export the chia primitives consumers/bindings need.
pub use chia_bls::{master_to_wallet_unhardened, PublicKey, SecretKey, Signature};
pub use chia_protocol::{Bytes, Bytes32, Coin, CoinSpend, Program, SpendBundle};
pub use chia_puzzle_types::{EveProof, LineageProof, Proof};
pub use chia_sdk_driver::{DataStore, DataStoreInfo, DataStoreMetadata, DelegatedPuzzle};

pub use error::{Error, WalletError};
pub use store::{
    add_fee, datastore_from_spend, digstore_owner_hint, hex_spend_bundle_to_coin_spends,
    melt_store, mint_store, oracle_spend, spend_bundle_to_hex, update_store_metadata,
    update_store_ownership, DataStoreInnerSpend, DATASTORE_LAUNCHER_HINT,
    DIGSTORE_OWNER_HINT_DOMAIN,
};
pub use types::SuccessResponse;
