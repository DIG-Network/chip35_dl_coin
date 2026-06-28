//! CAT (Chia Asset Token) single-issuance spend builder (roadmap #35).
//!
//! Issues a fixed-supply CAT via the genesis-by-coin-id TAIL (the parent coin id permanently fixes
//! the asset id; no further issuance is possible). Keyless: builds the coin spends; the caller signs.

use chia_bls::PublicKey;
use chia_protocol::{Bytes32, CoinSpend};
use chia_puzzle_types::standard::StandardArgs;
use chia_sdk_driver::{Cat, SpendContext, StandardLayer};
use chia_sdk_types::Conditions;

use crate::error::WalletError;

/// The result of issuing a CAT: the coin spends + the permanent asset id + the issued CAT coins.
#[derive(Clone, Debug)]
pub struct IssueCatResponse {
    /// Coin spends to be signed and broadcast.
    pub coin_spends: Vec<CoinSpend>,
    /// The CAT's asset id (TAIL hash) — permanent, derived from the genesis coin id.
    pub asset_id: Bytes32,
    /// The issued CAT coins (one per `amount` allocation; here a single coin of the full supply).
    pub cat_coins: Vec<chia_protocol::Coin>,
}

/// Build the spend bundle that issues a single-issuance (fixed-supply) CAT.
///
/// Issues `amount` units of a CAT whose TAIL is genesis-by-coin-id of the lead coin (so the supply
/// is permanently fixed). Spends `selected_coins` (first is lead; the rest assert concurrent spend),
/// reserves `fee`, and returns XCH change to the issuer. The full `amount` is minted to the issuer's
/// own puzzle hash. Returns the coin spends + the asset id + the CAT coins.
///
/// # Errors
/// [`WalletError::Parse`] if `selected_coins` is empty or their total is below `amount + fee`;
/// [`WalletError::Driver`] on spend-construction failure.
pub fn issue_cat(
    issuer_synthetic_key: PublicKey,
    selected_coins: Vec<chia_protocol::Coin>,
    amount: u64,
    fee: u64,
) -> Result<IssueCatResponse, WalletError> {
    if selected_coins.is_empty() {
        return Err(WalletError::Parse("selected_coins is empty".to_string()));
    }

    let issuer_puzzle_hash: Bytes32 = StandardArgs::curry_tree_hash(issuer_synthetic_key).into();
    let total_amount_from_coins: u64 = selected_coins.iter().map(|c| c.amount).sum();
    let reserved = amount + fee;
    if total_amount_from_coins < reserved {
        return Err(WalletError::Parse(
            "selected coins do not cover amount + fee".to_string(),
        ));
    }

    let mut ctx = SpendContext::new();
    let p2 = StandardLayer::new(issuer_synthetic_key);

    let lead_coin = selected_coins[0];
    let lead_coin_name = lead_coin.coin_id();

    for coin in selected_coins.into_iter().skip(1) {
        p2.spend(
            &mut ctx,
            coin,
            Conditions::new().assert_concurrent_spend(lead_coin_name),
        )?;
    }

    // Issue the CAT from the lead coin: the eve issuance returns the conditions the lead coin must
    // emit (mint the CAT to the issuer) plus the resulting CAT coins.
    let issuer_hint = ctx.hint(issuer_puzzle_hash)?;
    let (issue_conditions, cats) = Cat::issue_with_coin(
        &mut ctx,
        lead_coin_name,
        amount,
        Conditions::new().create_coin(issuer_puzzle_hash, amount, issuer_hint),
    )?;

    let mut lead_conditions = issue_conditions;
    if fee > 0 {
        lead_conditions = lead_conditions.reserve_fee(fee);
    }
    if total_amount_from_coins > reserved {
        lead_conditions = lead_conditions.create_coin(
            issuer_puzzle_hash,
            total_amount_from_coins - reserved,
            issuer_hint,
        );
    }
    p2.spend(&mut ctx, lead_coin, lead_conditions)?;

    let asset_id = cats
        .first()
        .map(|c| c.info.asset_id)
        .ok_or_else(|| WalletError::Parse("CAT issuance produced no coins".to_string()))?;

    Ok(IssueCatResponse {
        coin_spends: ctx.take(),
        asset_id,
        cat_coins: cats.iter().map(|c| c.coin).collect(),
    })
}
