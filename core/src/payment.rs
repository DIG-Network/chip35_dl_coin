//! In-dapp monetization payment + paywall primitives (roadmap #46).
//!
//! A dapp deployed on DIG can EARN: a buyer's wallet pays the dapp owner a specified amount in XCH
//! or any CAT (incl. DIG), settling to the owner's puzzle hash. This module builds the keyless
//! coin spends for that payment (the buyer signs), plus the verifiable receipt + check helper a
//! paywall (pay-to-unlock) uses to gate access on a confirmed payment.
//!
//! Boundary discipline (the same as every other builder here): no networking, no signing, no key
//! derivation, no coin selection. The caller supplies the buyer's selected coins (or CAT coins) and
//! the owner's puzzle hash; this builds the spends and returns a [`PaymentReceipt`] describing the
//! exact on-chain commitment so the dapp/SDK can verify it once the spend confirms.
//!
//! ## What "pay" means here
//! A payment CREATEs a coin paying `amount` to the owner's `puzzle_hash`, hinted to the owner (so the
//! owner's wallet sees it) and carrying a 32-byte `nonce` memo. The nonce ties an off-chain unlock
//! request to the exact on-chain coin: the dapp issues a nonce, the buyer pays with it, and the
//! paywall verifies a coin paying ≥ amount to the owner that carries that nonce. The nonce makes the
//! receipt unforgeable-by-replay (a different unlock request can't reuse a prior payment) without any
//! new on-chain machinery — it is just an indexed CREATE_COIN memo.
//!
//! ## XCH vs CAT
//! - **XCH**: spend the buyer's selected XCH coins, create the owner payment coin, reserve `fee`,
//!   return change. ([`build_xch_payment`])
//! - **CAT** (incl. DIG): ring-spend the buyer's selected CAT coins of one `asset_id`, create the
//!   owner CAT payment coin, return the CAT change to the buyer; an optional XCH `fee` is carried by
//!   a separate XCH coin via [`crate::add_fee`] (the CAT ring nets to zero and cannot pay an XCH
//!   fee). ([`build_cat_payment`])

use chia_bls::PublicKey;
use chia_protocol::{Bytes32, Coin, CoinSpend};
use chia_puzzle_types::standard::StandardArgs;
use chia_puzzle_types::Memos;
use chia_sdk_driver::{Cat, CatSpend, SpendContext, SpendWithConditions, StandardLayer};
use chia_sdk_types::Conditions;
use clvmr::NodePtr;

use crate::error::WalletError;

/// Which asset a payment settles in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaymentAsset {
    /// Native XCH (mojos).
    Xch,
    /// A CAT (incl. DIG), identified by its asset id (TAIL hash).
    Cat(Bytes32),
}

impl PaymentAsset {
    /// The CAT asset id, if this is a CAT payment.
    pub fn asset_id(&self) -> Option<Bytes32> {
        match self {
            PaymentAsset::Xch => None,
            PaymentAsset::Cat(id) => Some(*id),
        }
    }
}

/// A verifiable description of the on-chain commitment a payment makes. The dapp/SDK keeps this and,
/// once the spend confirms, checks the owner actually received a coin matching it (see
/// [`verify_payment_receipt`]). It is the receipt a paywall verifies — it pins WHO is paid (owner
/// puzzle hash), HOW MUCH, in WHICH asset, and the unlock NONCE.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentReceipt {
    /// The dapp owner's puzzle hash the payment settles to (the recipient).
    pub owner_puzzle_hash: Bytes32,
    /// The amount paid (mojos for XCH; CAT base units for a CAT).
    pub amount: u64,
    /// The asset paid in.
    pub asset: PaymentAsset,
    /// The 32-byte unlock nonce carried as a memo on the owner payment coin. Ties this payment to a
    /// specific off-chain unlock request so a paywall can match them and reject replays.
    pub nonce: Bytes32,
    /// The expected owner payment coin (the CREATE_COIN this build emits). Once the spend confirms,
    /// the dapp can look this coin up by id; its existence with the right puzzle_hash/amount IS the
    /// proof of payment.
    pub payment_coin: Coin,
}

/// The result of building a payment: the coin spends to sign + the receipt to verify after confirm.
#[derive(Clone, Debug)]
pub struct PaymentResponse {
    /// Coin spends to be signed by the buyer and broadcast.
    pub coin_spends: Vec<CoinSpend>,
    /// The receipt describing the on-chain commitment (feeds [`verify_payment_receipt`]).
    pub receipt: PaymentReceipt,
}

/// Encode the 32-byte unlock nonce as the single indexed memo on the owner payment coin.
///
/// The owner is the puzzle hash; hinting it to the owner (first memo) makes the coin discoverable by
/// the owner's wallet, and the nonce (second memo) is the paywall's match key. We emit
/// `[owner_puzzle_hash, nonce]` so a standard hinted-coin scan finds the coin AND the nonce travels
/// with it.
fn payment_memos(
    ctx: &mut SpendContext,
    owner_puzzle_hash: Bytes32,
    nonce: Bytes32,
) -> Result<Memos<NodePtr>, WalletError> {
    Ok(ctx.memos(&[owner_puzzle_hash, nonce])?)
}

/// Build the spends for a buyer to pay the dapp owner `amount` mojos of **XCH** (roadmap #46).
///
/// Spends the buyer's `selected_coins` (first is lead; the rest assert concurrent spend), creates the
/// owner payment coin (`amount` → `owner_puzzle_hash`, hinted to the owner, carrying `nonce`),
/// reserves `fee`, and returns change to the buyer. Returns the coin spends + the [`PaymentReceipt`].
///
/// # Errors
/// [`WalletError::Parse`] if `selected_coins` is empty or their total is below `amount + fee`;
/// [`WalletError::Driver`] on spend-construction failure.
pub fn build_xch_payment(
    buyer_synthetic_key: PublicKey,
    selected_coins: Vec<Coin>,
    owner_puzzle_hash: Bytes32,
    amount: u64,
    nonce: Bytes32,
    fee: u64,
) -> Result<PaymentResponse, WalletError> {
    if selected_coins.is_empty() {
        return Err(WalletError::Parse("selected_coins is empty".to_string()));
    }
    let buyer_puzzle_hash: Bytes32 = StandardArgs::curry_tree_hash(buyer_synthetic_key).into();
    let total: u64 = selected_coins.iter().map(|c| c.amount).sum();
    let reserved = amount + fee;
    if total < reserved {
        return Err(WalletError::Parse(
            "selected coins do not cover amount + fee".to_string(),
        ));
    }

    let mut ctx = SpendContext::new();
    let p2 = StandardLayer::new(buyer_synthetic_key);

    let lead_coin = selected_coins[0];
    let lead_coin_name = lead_coin.coin_id();
    for coin in selected_coins.into_iter().skip(1) {
        p2.spend(
            &mut ctx,
            coin,
            Conditions::new().assert_concurrent_spend(lead_coin_name),
        )?;
    }

    let pay_memos = payment_memos(&mut ctx, owner_puzzle_hash, nonce)?;
    let mut lead_conditions = Conditions::new().create_coin(owner_puzzle_hash, amount, pay_memos);
    if fee > 0 {
        lead_conditions = lead_conditions.reserve_fee(fee);
    }
    if total > reserved {
        let change_hint = ctx.hint(buyer_puzzle_hash)?;
        lead_conditions =
            lead_conditions.create_coin(buyer_puzzle_hash, total - reserved, change_hint);
    }
    p2.spend(&mut ctx, lead_coin, lead_conditions)?;

    // The owner payment coin is the child of the lead coin paying the owner.
    let payment_coin = Coin::new(lead_coin_name, owner_puzzle_hash, amount);

    Ok(PaymentResponse {
        coin_spends: ctx.take(),
        receipt: PaymentReceipt {
            owner_puzzle_hash,
            amount,
            asset: PaymentAsset::Xch,
            nonce,
            payment_coin,
        },
    })
}

/// Build the spends for a buyer to pay the dapp owner `amount` base units of a **CAT** (incl. DIG)
/// (roadmap #46).
///
/// Ring-spends the buyer's `selected_cats` (all of the SAME `asset_id`), creating one CAT coin of
/// `amount` to `owner_puzzle_hash` (hinted + carrying `nonce`) and returning the CAT change to the
/// buyer. The CAT ring nets to zero, so it cannot itself pay an XCH network fee — carry a fee with a
/// separate XCH coin via [`crate::add_fee`], asserting the lead CAT coin id. Returns the coin spends
/// + the [`PaymentReceipt`].
///
/// `selected_cats` are the buyer's existing [`Cat`] coins (parsed on-chain by the caller). They must
/// all share `asset_id` and their total amount must be ≥ `amount`.
///
/// # Errors
/// [`WalletError::Parse`] if `selected_cats` is empty, mixes asset ids, or totals below `amount`;
/// [`WalletError::Driver`] on spend-construction failure.
pub fn build_cat_payment(
    buyer_synthetic_key: PublicKey,
    selected_cats: Vec<Cat>,
    owner_puzzle_hash: Bytes32,
    amount: u64,
    nonce: Bytes32,
) -> Result<PaymentResponse, WalletError> {
    if selected_cats.is_empty() {
        return Err(WalletError::Parse("selected_cats is empty".to_string()));
    }
    let asset_id = selected_cats[0].info.asset_id;
    if selected_cats.iter().any(|c| c.info.asset_id != asset_id) {
        return Err(WalletError::Parse(
            "selected_cats mix more than one asset id".to_string(),
        ));
    }
    let buyer_puzzle_hash: Bytes32 = StandardArgs::curry_tree_hash(buyer_synthetic_key).into();
    let total: u64 = selected_cats.iter().map(|c| c.coin.amount).sum();
    if total < amount {
        return Err(WalletError::Parse(
            "selected CATs do not cover amount".to_string(),
        ));
    }

    let mut ctx = SpendContext::new();
    let p2 = StandardLayer::new(buyer_synthetic_key);

    // The lead CAT carries the payment + change CREATE_COINs; the rest emit nothing (the CAT ring
    // accounts for their value via the subtotals computed in `Cat::spend_all`).
    let lead_coin_name = selected_cats[0].coin.coin_id();
    let pay_memos = payment_memos(&mut ctx, owner_puzzle_hash, nonce)?;
    let change = total - amount;

    let mut cat_spends = Vec::with_capacity(selected_cats.len());
    for (i, cat) in selected_cats.iter().enumerate() {
        let conditions = if i == 0 {
            let mut c = Conditions::new().create_coin(owner_puzzle_hash, amount, pay_memos);
            if change > 0 {
                let change_hint = ctx.hint(buyer_puzzle_hash)?;
                c = c.create_coin(buyer_puzzle_hash, change, change_hint);
            }
            c
        } else {
            Conditions::new().assert_concurrent_spend(lead_coin_name)
        };
        let inner = p2.spend_with_conditions(&mut ctx, conditions)?;
        cat_spends.push(CatSpend::new(*cat, inner));
    }
    Cat::spend_all(&mut ctx, &cat_spends)?;

    // The owner CAT payment coin's puzzle hash is the CAT outer puzzle wrapping the owner's inner p2.
    let payment_coin = selected_cats[0].child(owner_puzzle_hash, amount).coin;

    Ok(PaymentResponse {
        coin_spends: ctx.take(),
        receipt: PaymentReceipt {
            owner_puzzle_hash,
            amount,
            asset: PaymentAsset::Cat(asset_id),
            nonce,
            payment_coin,
        },
    })
}

/// What an observed payment looks like to the verifier (the paywall) once it has read the chain.
///
/// The dapp/SDK fetches the on-chain coin that was created for the owner (by coin id from the receipt,
/// or by scanning the owner's hinted coins for the nonce) and fills this in from what it observed. The
/// asset is resolved by the verifier: an XCH coin → [`PaymentAsset::Xch`]; a CAT coin whose asset id
/// it recovered → [`PaymentAsset::Cat`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedPayment {
    /// The puzzle hash the observed coin actually pays.
    pub paid_to_puzzle_hash: Bytes32,
    /// The observed amount.
    pub amount: u64,
    /// The observed asset.
    pub asset: PaymentAsset,
    /// The 32-byte nonce memo observed on the coin (if any).
    pub nonce: Option<Bytes32>,
}

/// The reasons a paywall check can fail — actionable so the dapp/SDK can tell the user precisely
/// what is wrong rather than a generic "access denied".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaywallError {
    /// The coin pays a different puzzle hash than the required owner.
    WrongRecipient { expected: Bytes32, got: Bytes32 },
    /// The observed amount is below the required minimum.
    InsufficientAmount { required: u64, got: u64 },
    /// The observed asset is not the required asset.
    WrongAsset {
        required: PaymentAsset,
        got: PaymentAsset,
    },
    /// The observed nonce does not match the unlock request's nonce.
    NonceMismatch {
        expected: Bytes32,
        got: Option<Bytes32>,
    },
}

impl PaywallError {
    /// A stable, machine-readable `UPPER_SNAKE` code a dapp/agent can branch on instead of
    /// string-matching the human [`Display`] message. Part of the public contract (changing one is a
    /// breaking change); surfaced as the `code` field of the `{ ok:false, code, error }` result the
    /// wasm `verifyPaymentReceipt` helper returns.
    pub fn code(&self) -> &'static str {
        match self {
            PaywallError::WrongRecipient { .. } => "WRONG_RECIPIENT",
            PaywallError::InsufficientAmount { .. } => "INSUFFICIENT_AMOUNT",
            PaywallError::WrongAsset { .. } => "WRONG_ASSET",
            PaywallError::NonceMismatch { .. } => "NONCE_MISMATCH",
        }
    }
}

impl core::fmt::Display for PaywallError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PaywallError::WrongRecipient { expected, got } => {
                write!(
                    f,
                    "payment recipient {got} is not the required owner {expected}"
                )
            }
            PaywallError::InsufficientAmount { required, got } => {
                write!(f, "payment amount {got} is below the required {required}")
            }
            PaywallError::WrongAsset { required, got } => {
                write!(f, "payment asset {got:?} is not the required {required:?}")
            }
            PaywallError::NonceMismatch { expected, got } => {
                write!(
                    f,
                    "payment nonce {got:?} does not match the expected {expected}"
                )
            }
        }
    }
}

/// Verify an observed payment unlocks a paywall: it must pay the required `owner_puzzle_hash`, in the
/// required `asset`, at least `min_amount`, and (when `require_nonce` is set) carry that nonce
/// (roadmap #46, pay-to-unlock).
///
/// This is the check helper a dapp/SDK runs after the spend confirms: it reads the owner's coin from
/// the chain (by the receipt's coin id, or by scanning the owner's hinted coins for the nonce), fills
/// in an [`ObservedPayment`], and calls this. `Ok(())` means access is granted; an [`Err`] explains
/// exactly which gate failed. Pass `require_nonce = Some(nonce)` to bind the unlock to a specific
/// off-chain request (the recommended mode — it defeats replay of an unrelated payment); pass `None`
/// to accept any payment meeting the amount/asset/recipient gate.
pub fn verify_payment_receipt(
    observed: &ObservedPayment,
    owner_puzzle_hash: Bytes32,
    min_amount: u64,
    asset: PaymentAsset,
    require_nonce: Option<Bytes32>,
) -> Result<(), PaywallError> {
    if observed.paid_to_puzzle_hash != owner_puzzle_hash {
        return Err(PaywallError::WrongRecipient {
            expected: owner_puzzle_hash,
            got: observed.paid_to_puzzle_hash,
        });
    }
    if observed.asset != asset {
        return Err(PaywallError::WrongAsset {
            required: asset,
            got: observed.asset,
        });
    }
    if observed.amount < min_amount {
        return Err(PaywallError::InsufficientAmount {
            required: min_amount,
            got: observed.amount,
        });
    }
    if let Some(expected) = require_nonce {
        if observed.nonce != Some(expected) {
            return Err(PaywallError::NonceMismatch {
                expected,
                got: observed.nonce,
            });
        }
    }
    Ok(())
}

/// Derive a 32-byte unlock nonce from arbitrary request bytes (e.g. `dapp_id || resource || user`).
///
/// A dapp issues one nonce per unlock request and embeds it in the payment via [`build_xch_payment`]/
/// [`build_cat_payment`], then verifies it with [`verify_payment_receipt`]. Using a hash of the
/// request bytes makes the nonce deterministic and collision-resistant without the dapp storing
/// random state. (The dapp may also use any random 32 bytes — this is a convenience, not a
/// requirement.)
pub fn payment_nonce(request_bytes: &[u8]) -> Bytes32 {
    crate::metadata::sha256(request_bytes)
}
