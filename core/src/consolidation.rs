//! Coin-consolidation spend builders (epic #410 / #413).
//!
//! When a wallet holds too many small coins to cover a target within the selection cap
//! ([`crate::SelectCoinsResult::NeedsConsolidation`]), it must first MERGE coins into fewer, larger
//! ones. These builders produce that merge as a keyless self-send: up to `cap` of the SMALLEST
//! (most-fragmenting) coins of ONE asset collapse into a SINGLE output back to the owner's own puzzle
//! hash. The caller signs + pushes the returned spends, waits for confirmation, then re-selects; the
//! client's auto-combine loop repeats until a selection fits (or the user cancels).
//!
//! Boundary discipline (same as [`crate::payment`]): no networking, no signing, no key derivation.
//! The owner's puzzle hash is derived from the provided synthetic key (a true self-send), and every
//! input coin must be spendable by that one key — matching the existing single-key builders.
//!
//! - **XCH** ([`build_coin_consolidation`]): spend the smallest `cap` coins, create ONE output of
//!   `total - fee` to the owner, reserve an optional `fee`.
//! - **CAT** ([`build_cat_consolidation`]): ring-spend the smallest `cap` CAT coins of one asset id,
//!   create ONE output CAT coin of `total` to the owner. A CAT ring nets to zero and cannot pay an
//!   XCH network fee — carry one on a separate XCH coin via [`crate::add_fee`] asserting the lead CAT
//!   coin id, exactly as CAT payments do.

use chia_bls::PublicKey;
use chia_protocol::{Bytes32, Coin, CoinSpend};
use chia_puzzle_types::standard::StandardArgs;
use chia_sdk_driver::{Cat, CatSpend, SpendContext, SpendWithConditions, StandardLayer};
use chia_sdk_types::Conditions;

use crate::error::WalletError;

/// Pick the smallest `cap` items (most-fragmenting first) via a deterministic sort key, requiring at
/// least two to merge. Returns the retained prefix. `key` maps an item to `(amount, coin_id)`.
fn smallest_cap<T, F>(mut items: Vec<T>, cap: usize, key: F) -> Result<Vec<T>, WalletError>
where
    F: Fn(&T) -> (u64, Bytes32),
{
    if items.len() < 2 {
        return Err(WalletError::Parse(
            "consolidation requires at least 2 coins".to_string(),
        ));
    }
    // Smallest amount first, tie-break by coin id ascending — deterministic.
    items.sort_by(|a, b| {
        let (aa, ai) = key(a);
        let (ba, bi) = key(b);
        aa.cmp(&ba).then_with(|| ai.cmp(&bi))
    });
    let take = cap.min(items.len());
    if take < 2 {
        return Err(WalletError::Parse(
            "cap must allow merging at least 2 coins".to_string(),
        ));
    }
    items.truncate(take);
    Ok(items)
}

/// Build the keyless spends that merge up to `cap` of the smallest XCH `coins` into ONE output coin
/// back to the owner (self-send of `total - fee`), reserving an optional network `fee` (epic #410).
///
/// The owner is `StandardArgs::curry_tree_hash(spender_synthetic_key)`; every input coin must be
/// spendable by that key. Requires at least two coins.
///
/// # Errors
/// [`WalletError::Parse`] if fewer than two coins are eligible, or `fee >= total`;
/// [`WalletError::Driver`] on spend-construction failure.
pub fn build_coin_consolidation(
    spender_synthetic_key: PublicKey,
    coins: Vec<Coin>,
    cap: usize,
    fee: u64,
) -> Result<Vec<CoinSpend>, WalletError> {
    let merge = smallest_cap(coins, cap, |c| (c.amount, c.coin_id()))?;
    let total = merge
        .iter()
        .map(|c| c.amount)
        .fold(0u64, u64::saturating_add);
    if total <= fee {
        return Err(WalletError::Parse(
            "fee must be less than the consolidated amount".to_string(),
        ));
    }

    let owner_puzzle_hash: Bytes32 = StandardArgs::curry_tree_hash(spender_synthetic_key).into();
    let mut ctx = SpendContext::new();
    let p2 = StandardLayer::new(spender_synthetic_key);

    // The smallest coin is the lead: it carries the single consolidated CREATE_COIN + the fee; the
    // rest just assert concurrent spend so they are consumed atomically.
    let lead = merge[0];
    let lead_id = lead.coin_id();
    for coin in merge.iter().skip(1) {
        p2.spend(
            &mut ctx,
            *coin,
            Conditions::new().assert_concurrent_spend(lead_id),
        )?;
    }

    let hint = ctx.hint(owner_puzzle_hash)?;
    let mut lead_conditions = Conditions::new().create_coin(owner_puzzle_hash, total - fee, hint);
    if fee > 0 {
        lead_conditions = lead_conditions.reserve_fee(fee);
    }
    p2.spend(&mut ctx, lead, lead_conditions)?;

    Ok(ctx.take())
}

/// Build the keyless spends that merge up to `cap` of the smallest CAT `cats` (all of ONE asset id)
/// into ONE output CAT coin of `total` back to the owner (self-send) (epic #410).
///
/// The CAT ring nets to zero, so this reserves NO fee — carry an XCH network fee on a separate coin
/// via [`crate::add_fee`] asserting the lead CAT coin id. The owner is
/// `StandardArgs::curry_tree_hash(spender_synthetic_key)`; every input CAT must be spendable by that
/// key. Requires at least two CATs of the same asset id.
///
/// # Errors
/// [`WalletError::Parse`] if fewer than two CATs are eligible or they mix asset ids;
/// [`WalletError::Driver`] on spend-construction failure.
pub fn build_cat_consolidation(
    spender_synthetic_key: PublicKey,
    cats: Vec<Cat>,
    cap: usize,
) -> Result<Vec<CoinSpend>, WalletError> {
    if let Some(first) = cats.first() {
        let asset_id = first.info.asset_id;
        if cats.iter().any(|c| c.info.asset_id != asset_id) {
            return Err(WalletError::Parse(
                "selected CATs mix more than one asset id".to_string(),
            ));
        }
    }
    let merge = smallest_cap(cats, cap, |c| (c.coin.amount, c.coin.coin_id()))?;
    let total = merge
        .iter()
        .map(|c| c.coin.amount)
        .fold(0u64, u64::saturating_add);

    let owner_puzzle_hash: Bytes32 = StandardArgs::curry_tree_hash(spender_synthetic_key).into();
    let mut ctx = SpendContext::new();
    let p2 = StandardLayer::new(spender_synthetic_key);

    // The lead CAT carries the single consolidated CREATE_COIN; the rest emit nothing (the CAT ring
    // accounts for their value via the subtotals `Cat::spend_all` computes).
    let lead_id = merge[0].coin.coin_id();
    let mut cat_spends = Vec::with_capacity(merge.len());
    for (i, cat) in merge.iter().enumerate() {
        let conditions = if i == 0 {
            let hint = ctx.hint(owner_puzzle_hash)?;
            Conditions::new().create_coin(owner_puzzle_hash, total, hint)
        } else {
            Conditions::new().assert_concurrent_spend(lead_id)
        };
        let inner = p2.spend_with_conditions(&mut ctx, conditions)?;
        cat_spends.push(CatSpend::new(*cat, inner));
    }
    Cat::spend_all(&mut ctx, &cat_spends)?;

    Ok(ctx.take())
}
