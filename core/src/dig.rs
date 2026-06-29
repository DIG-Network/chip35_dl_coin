//! The DIG-CAT per-capsule payment — the canonical store-payment spend builder.
//!
//! ## Pricing model: mint is FREE of $DIG; a CAPSULE (commit) is paid
//! Creating (minting) a data **store** does NOT cost $DIG — minting only launches the on-chain
//! DataLayer singleton with an empty/initial root and pays the XCH network fee. $DIG is paid ONLY
//! when a **capsule** is created: every commit / root-advance (a new `(storeId, rootHash)` generation)
//! pays the per-capsule price in $DIG to the DIG treasury. So `mint_store` (the launch) carries NO DIG
//! payment, and the COMMIT path concatenates a [`build_dig_store_payment`] CAT spend with the
//! root-advance ([`crate::update_store_metadata`]) into one atomic bundle.
//!
//! ## Dynamic, USD-pegged amount — the caller passes it in
//! The per-capsule price is **dynamic and USD-pegged** (SYSTEM.md → Core concept → Pricing):
//! `dig_amount = target_usd ÷ live DIG price`, uniform per fixed-size capsule. This builder is
//! offline + deterministic and NEVER fetches a price — the live `amount` (base units) is an input
//! param computed by the caller (the hub in-browser, the CLI from `--dig-amount`). There is
//! intentionally NO hardcoded amount constant here.
//!
//! ## Shared contract — byte-identical across the ecosystem
//! The DIG CAT asset id, the treasury inner puzzle hash, and the treasury-output memo layout
//! (`[treasury_inner_ph (hint), store_id]`) are a cross-system shared contract (SYSTEM.md →
//! Shared contracts → "DIG CAT payment"). They MUST be byte-identical to the digstore-chain mirror
//! (`crates/digstore-chain/src/{dig,cat}.rs`). A drift here breaks payment atomicity / anchor-watcher
//! gating. chip35 is the canonical spend builder, so this is the source the mirror follows.
//!
//! Boundary discipline (the same as every builder here): no networking, no signing, no key
//! derivation, no coin selection. The caller supplies the buyer's reconstructed DIG [`Cat`] coins and
//! the launcher id; this builds the (unsigned) CAT coin spends. The caller concatenates them with the
//! commit's singleton spend and signs the whole bundle.

use chia_bls::PublicKey;
use chia_protocol::{Bytes32, Coin, CoinSpend};
use chia_puzzle_types::standard::StandardArgs;
use chia_sdk_driver::{Cat, CatSpend, SpendContext, SpendWithConditions, StandardLayer};
use chia_sdk_types::Conditions;
use hex_literal::hex;

use crate::error::WalletError;

/// DIG CAT asset id (TAIL hash) on Chia **mainnet**. The single token a capsule (commit) is paid in.
///
/// CONTRACT: byte-identical to digstore-chain's `DIG_ASSET_ID` and DataLayer-Driver's. Do not change
/// without changing every consumer in lockstep (SYSTEM.md → Shared contracts → DIG CAT payment).
pub const DIG_ASSET_ID: Bytes32 = Bytes32::new(hex!(
    "a406d3a9de984d03c9591c10d917593b434d5263cabe2b42f6b367df16832f81"
));

/// The DIG treasury's INNER (standard) puzzle hash — the recipient of every per-capsule DIG payment.
///
/// This is the inner puzzle hash decoded from the treasury address
/// `xch1a37rq3cgcl2ecpudttsf35x75qzdan68lgw2l6ajvmqs44jxdn5qv6pk3y`; the CAT layer (curried with
/// [`DIG_ASSET_ID`]) wraps it so the on-chain coin lands at the treasury's DIG CAT puzzle hash.
///
/// CONTRACT: byte-identical to the digstore-chain mirror (`treasury_inner_puzzle_hash()` decodes the
/// SAME address) and the hub's `DIG_TREASURY_*` constants. The anchor-watcher gates confirmed payments
/// on a coin landing at the DIG-CAT wrap of THIS hash, so a drift silently rejects valid payments.
pub const DIG_TREASURY_INNER_PUZZLE_HASH: Bytes32 = Bytes32::new(hex!(
    "ec7c304708c7d59c078d5ae098d0dea004decf47fa1cafebb266c10ad6466ce8"
));

/// Build the (UNSIGNED) DIG-CAT coin spends that pay `amount` base units of $DIG to the DIG treasury
/// for a capsule (commit) — the canonical per-capsule store payment.
///
/// This is the ONLY place a store payment is constructed (chip35 is the canonical spend builder). The
/// COMMIT path concatenates the returned coin spends with the root-advance singleton spend
/// ([`crate::update_store_metadata`]) into ONE bundle and signs them together, so the payment and the
/// new capsule are admitted atomically. The MINT path does NOT call this — minting a store is free of
/// $DIG.
///
/// Behaviour (byte-mirror of digstore-chain's `build_dig_payment`):
/// - ring-spends the buyer's `dig_cats` (all of [`DIG_ASSET_ID`]),
/// - creates ONE DIG CAT coin of `amount` to [`DIG_TREASURY_INNER_PUZZLE_HASH`], carrying memos
///   `[treasury_inner_ph (hint), store_id]` so the treasury wallet discovers it and the payment is
///   tied to the capsule's store,
/// - returns the DIG change to the buyer (hinted to their own inner puzzle hash),
/// - reserves NO XCH (the CAT ring nets to zero; an XCH network fee rides on a separate coin via the
///   commit's [`crate::update_store_metadata`] + [`crate::add_fee`]).
///
/// `amount` is the dynamic, USD-pegged per-capsule price in DIG base units — an INPUT, never a
/// hardcoded constant (see the module docs). `store_id` is the capsule's launcher id (= store id).
///
/// # Errors
/// [`WalletError::Parse`] if `dig_cats` is empty, mixes asset ids, is not the DIG asset, or totals
/// below `amount`; [`WalletError::Driver`] on spend-construction failure.
pub fn build_dig_store_payment(
    buyer_synthetic_key: PublicKey,
    dig_cats: Vec<Cat>,
    store_id: Bytes32,
    amount: u64,
) -> Result<Vec<CoinSpend>, WalletError> {
    if dig_cats.is_empty() {
        return Err(WalletError::Parse("dig_cats is empty".to_string()));
    }
    let asset_id = dig_cats[0].info.asset_id;
    if asset_id != DIG_ASSET_ID {
        return Err(WalletError::Parse(
            "dig_cats are not the DIG asset".to_string(),
        ));
    }
    if dig_cats.iter().any(|c| c.info.asset_id != asset_id) {
        return Err(WalletError::Parse(
            "dig_cats mix more than one asset id".to_string(),
        ));
    }
    let buyer_puzzle_hash: Bytes32 = StandardArgs::curry_tree_hash(buyer_synthetic_key).into();
    let total: u64 = dig_cats.iter().map(|c| c.coin.amount).sum();
    if total < amount {
        return Err(WalletError::Parse(
            "selected DIG does not cover amount".to_string(),
        ));
    }

    let mut ctx = SpendContext::new();
    let p2 = StandardLayer::new(buyer_synthetic_key);

    // The lead CAT carries the treasury payment + change CREATE_COINs; the rest emit nothing (the CAT
    // ring accounts for their value via the subtotals computed in `Cat::spend_all`).
    let lead_coin_name = dig_cats[0].coin.coin_id();
    // Treasury output memos: [treasury inner ph (hint), store id] — byte-mirror of digstore-chain.
    let pay_memos = ctx.memos(&[DIG_TREASURY_INNER_PUZZLE_HASH, store_id])?;
    let change = total - amount;

    let mut cat_spends = Vec::with_capacity(dig_cats.len());
    for (i, cat) in dig_cats.iter().enumerate() {
        let conditions = if i == 0 {
            let mut c =
                Conditions::new().create_coin(DIG_TREASURY_INNER_PUZZLE_HASH, amount, pay_memos);
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

    Ok(ctx.take())
}

/// The DIG CAT payment coin a capsule (commit) pays to the treasury — what
/// [`build_dig_store_payment`] emits.
///
/// `parent` is the lead DIG CAT coin's id; the returned coin is the treasury's DIG CAT coin (the CAT
/// outer puzzle wrapping the treasury inner puzzle hash). Lets a caller pin the exact expected coin
/// (e.g. for a payment receipt / verification) without re-deriving the CAT wrap by hand.
pub fn dig_treasury_payment_coin(lead_dig_cat: &Cat, amount: u64) -> Coin {
    lead_dig_cat
        .child(DIG_TREASURY_INNER_PUZZLE_HASH, amount)
        .coin
}
