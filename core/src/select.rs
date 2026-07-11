//! Shared high-value-first coin selection with a coin-count cap (epic #410 / #413).
//!
//! Every browser/JS spend creator (hub, extension, dig-sdk) needs the SAME selection policy so a
//! wallet moves value consistently and a "too many coins" condition is signalled — not hidden behind
//! a generic insufficient-funds error. This is that one policy, implemented once here (released via
//! the chip35 wasm) and byte-mirrored by the native Rust layer (`dig-l1-wallet`, T0b/#414).
//!
//! ## Policy
//! Sort the available coins DESCENDING by amount (tie-break by coin id, ascending, for determinism),
//! then greedily accumulate largest-first until the running total reaches `target`. Only a selection
//! that fits within `cap` coins is accepted. The outcome is one of three DISTINCT results
//! ([`SelectCoinsResult`]):
//! - [`SelectCoinsResult::Ok`] — a selection within `cap` covers the target.
//! - [`SelectCoinsResult::NeedsConsolidation`] — enough total value EXISTS, but reaching the target
//!   needs more than `cap` coins. The caller consolidates (merge coins → one, see
//!   [`crate::build_coin_consolidation`] / [`crate::build_cat_consolidation`]) then re-selects.
//! - [`SelectCoinsResult::InsufficientFunds`] — the total value is below the target regardless of the
//!   cap. This is genuinely not-enough-money, kept separate from the consolidation signal.
//!
//! Boundary discipline: pure. No keys, no networking, no on-chain reads. Works for XCH and for any
//! single CAT tail (the caller passes the CAT coins' underlying [`Coin`]s).

use chia_protocol::Coin;

/// The default maximum number of coins a single selection (and thus a single spend bundle) will use.
/// Chosen to keep a spend bundle comfortably within block/mempool cost limits; a caller may override
/// it per selection.
pub const DEFAULT_COIN_CAP: usize = 50;

/// A successful selection: the chosen coins (descending by amount, tie-broken by coin id) plus the
/// selected total and the change over `target`. `coins[0]` is the largest — the natural lead coin for
/// the builders, which spend element 0 as the lead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoinSelection {
    /// The chosen coins, ordered largest-first (deterministic).
    pub coins: Vec<Coin>,
    /// The sum of the chosen coins' amounts (`>= target`).
    pub total: u64,
    /// The change over the target (`total - target`).
    pub change: u64,
}

/// The discriminated outcome of [`select_coins`]. `NeedsConsolidation` is deliberately distinct from
/// `InsufficientFunds`: the former means "the money is here but too fragmented", the latter means
/// "not enough money". Every failure carries the counts the caller needs to drive a consolidation
/// loop or an honest error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectCoinsResult {
    /// A selection within `cap` covers the target.
    Ok(CoinSelection),
    /// Enough total value exists, but covering `target` needs more than `cap` coins — consolidate
    /// (merge coins into fewer, larger ones), then re-select.
    NeedsConsolidation {
        /// How many coins the wallet holds for this asset.
        available_coin_count: usize,
        /// The wallet's total value for this asset (saturating).
        available_total: u64,
        /// The target that was requested.
        required: u64,
        /// The cap that was applied.
        cap: usize,
    },
    /// The total value is below `target` — genuinely insufficient funds (independent of the cap).
    InsufficientFunds {
        /// How many coins the wallet holds for this asset.
        available_coin_count: usize,
        /// The wallet's total value for this asset (saturating).
        available_total: u64,
        /// The target that was requested.
        required: u64,
        /// The cap that was applied.
        cap: usize,
    },
}

/// Select coins high-value-first to cover `target`, using at most `cap` coins.
///
/// See the [module docs](self) for the policy and the three-way result. Deterministic for a given
/// input set + `target` + `cap`. Pure — no keys, no networking.
pub fn select_coins(coins: Vec<Coin>, target: u64, cap: usize) -> SelectCoinsResult {
    let available_coin_count = coins.len();
    let available_total = coins
        .iter()
        .map(|c| c.amount)
        .fold(0u64, u64::saturating_add);

    // Not enough value even using every coin → insufficient (distinct from needs-consolidation).
    if available_total < target {
        return SelectCoinsResult::InsufficientFunds {
            available_coin_count,
            available_total,
            required: target,
            cap,
        };
    }

    // Deterministic order: descending by amount, tie-break by coin id ascending.
    let mut sorted = coins;
    sorted.sort_by(|a, b| {
        b.amount
            .cmp(&a.amount)
            .then_with(|| a.coin_id().cmp(&b.coin_id()))
    });

    // Greedy accumulate largest-first until the running total reaches the target. Because
    // `available_total >= target`, this always reaches it before the coins run out.
    let mut chosen: Vec<Coin> = Vec::new();
    let mut total: u64 = 0;
    for coin in sorted {
        if total >= target {
            break;
        }
        total = total.saturating_add(coin.amount);
        chosen.push(coin);
    }

    if chosen.len() > cap {
        // The value exists but reaching the target needs more than `cap` coins → consolidate.
        return SelectCoinsResult::NeedsConsolidation {
            available_coin_count,
            available_total,
            required: target,
            cap,
        };
    }

    SelectCoinsResult::Ok(CoinSelection {
        change: total - target,
        total,
        coins: chosen,
    })
}
