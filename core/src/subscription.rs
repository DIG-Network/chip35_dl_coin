//! Subscription / recurring-payment model (roadmap #46) — SCAFFOLD, not yet implemented.
//!
//! A one-shot [`payment`](crate::payment) is a single CREATE_COIN the buyer signs now. A
//! SUBSCRIPTION is fundamentally more than a spend builder: it is a standing authorization to pull a
//! recurring amount on a schedule, which on Chia means real puzzle machinery — none of which exists
//! in `chia-sdk-driver` 0.30 as a ready primitive, so it is deliberately NOT faked here.
//!
//! ## Why it needs more than a spend builder
//! Chia has no native "recurring payment". A real subscription needs one of:
//! - a **time-locked / clawback puzzle** (e.g. `ASSERT_SECONDS_RELATIVE` gating each period's
//!   release) the buyer funds once, from which the dapp claims one period at a time after each
//!   interval elapses — plus a buyer clawback path to cancel and reclaim the unclaimed remainder; or
//! - a **pre-authorized delegated-spend puzzle** (a curried max-per-period + period length + payee)
//!   the buyer signs once, that the dapp re-spends each period — i.e. a bespoke chialisp puzzle that
//!   must be written, audited, and shipped inside (or alongside) the driver before it can be built.
//!
//! Both are new on-chain puzzles requiring chialisp + a security review (the same bar that retired the
//! hand-rolled `deploy_token` puzzle in favour of native CHIP-0035 delegation). Until one lands, the
//! honest answer for "recurring" is: **the dapp issues a fresh one-shot [`payment`](crate::payment)
//! per period** (the buyer approves each renewal), and a paywall gates on the latest period's receipt
//! ([`crate::payment::verify_payment_receipt`]). That is real, shippable, and requires no new puzzle.
//!
//! When the recurring puzzle is designed, the builders below get real bodies; their signatures sketch
//! the intended shape so consumers (dig-sdk/hub) know what is coming.

use chia_bls::PublicKey;
use chia_protocol::{Bytes32, Coin, CoinSpend};

use crate::error::WalletError;

/// The terms of a subscription a buyer authorizes once: pay `amount_per_period` of `asset` to
/// `payee_puzzle_hash` every `period_seconds`, for at most `max_periods`.
///
/// This is the data shape the future builder will curry into the time-locked / delegated-spend puzzle.
/// It is defined now so consumers can model subscriptions ahead of the on-chain implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubscriptionTerms {
    /// The dapp owner's puzzle hash each period pays to.
    pub payee_puzzle_hash: Bytes32,
    /// Amount per period (mojos for XCH; base units for a CAT).
    pub amount_per_period: u64,
    /// CAT asset id, or `None` for XCH.
    pub asset_id: Option<Bytes32>,
    /// Seconds between claimable periods.
    pub period_seconds: u64,
    /// Maximum number of periods the authorization covers (caps total spend).
    pub max_periods: u32,
}

/// TODO(#46 subscriptions): build the one-time spend that FUNDS a subscription — locking
/// `terms.amount_per_period * terms.max_periods` into a time-locked / pre-authorized puzzle the dapp
/// can claim one period at a time, with a buyer clawback path. Requires the recurring puzzle (see the
/// module docs); NOT yet implemented. Returns [`WalletError::Parse`] until the puzzle ships.
///
/// Until then, model recurring billing as a fresh one-shot [`crate::payment::build_xch_payment`] /
/// [`build_cat_payment`](crate::payment::build_cat_payment) per period.
pub fn build_subscription_authorization(
    _buyer_synthetic_key: PublicKey,
    _selected_coins: Vec<Coin>,
    _terms: SubscriptionTerms,
    _fee: u64,
) -> Result<Vec<CoinSpend>, WalletError> {
    Err(WalletError::Parse(
        "subscriptions are not yet implemented: they require a time-locked/delegated recurring \
         puzzle (chialisp + security review). Model recurring billing as a one-shot payment per \
         period via build_xch_payment / build_cat_payment. See subscription.rs module docs."
            .to_string(),
    ))
}

/// TODO(#46 subscriptions): build the spend the DAPP runs each period to CLAIM that period's
/// `amount_per_period` from a funded subscription (after `period_seconds` elapsed). Requires the
/// recurring puzzle; NOT yet implemented.
pub fn build_subscription_claim(
    _payee_synthetic_key: PublicKey,
    _subscription_coin: Coin,
    _terms: SubscriptionTerms,
) -> Result<Vec<CoinSpend>, WalletError> {
    Err(WalletError::Parse(
        "subscriptions are not yet implemented (claim path). See subscription.rs module docs."
            .to_string(),
    ))
}
