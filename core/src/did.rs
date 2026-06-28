//! DID (creator identity) spend builder (roadmap #35 / #38).
//!
//! Creates a Chia DID singleton — a verifiable creator identity collectors can check on a minted
//! NFT. Keyless: builds the coin spends; the caller signs. The DID's launcher id + inner puzzle
//! hash feed [`crate::nft::DidAttribution`] to attribute mints.

use chia_bls::PublicKey;
use chia_protocol::{Bytes32, Coin, CoinSpend};
use chia_puzzle_types::standard::StandardArgs;
use chia_sdk_driver::{Launcher, SingletonInfo, SpendContext, StandardLayer};
use chia_sdk_types::Conditions;

use crate::error::WalletError;

/// The result of creating a DID: the coin spends + the new DID's identifiers for storage/attribution.
#[derive(Clone, Debug)]
pub struct CreateDidResponse {
    /// Coin spends to be signed and broadcast.
    pub coin_spends: Vec<CoinSpend>,
    /// The DID's launcher id (its permanent identifier — the "did:chia:..." root).
    pub launcher_id: Bytes32,
    /// The DID's current inner puzzle hash (needed to attribute mints to it).
    pub inner_puzzle_hash: Bytes32,
    /// The DID coin after creation (for chaining further spends).
    pub did_coin: Coin,
}

/// Build the spend bundle that creates a simple DID (one verification, no recovery list) owned by
/// the minter.
///
/// Spends `selected_coins` (first is lead; the rest assert concurrent spend), launches the DID
/// singleton from the lead coin, reserves `fee`, and returns change to the minter. Returns the coin
/// spends + the DID identifiers.
///
/// # Errors
/// [`WalletError::Parse`] if `selected_coins` is empty; [`WalletError::Driver`] on spend-construction
/// failure.
pub fn create_did(
    minter_synthetic_key: PublicKey,
    selected_coins: Vec<Coin>,
    fee: u64,
) -> Result<CreateDidResponse, WalletError> {
    if selected_coins.is_empty() {
        return Err(WalletError::Parse("selected_coins is empty".to_string()));
    }

    let minter_puzzle_hash: Bytes32 = StandardArgs::curry_tree_hash(minter_synthetic_key).into();
    let total_amount_from_coins: u64 = selected_coins.iter().map(|c| c.amount).sum();
    let reserved = fee + 1; // 1 mojo funds the DID launcher.

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

    let (create_did, did) = Launcher::new(lead_coin_name, 1).create_simple_did(&mut ctx, &p2)?;

    let mut lead_conditions = create_did;
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

    Ok(CreateDidResponse {
        coin_spends: ctx.take(),
        launcher_id: did.info.launcher_id,
        inner_puzzle_hash: did.info.inner_puzzle_hash().into(),
        did_coin: did.coin,
    })
}
