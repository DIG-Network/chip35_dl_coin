//! Tests for the shared coin-selection + coin-consolidation primitives (epic #410 / #413).
//!
//! `select_coins` is pure (no keys, no networking); the consolidation builders are keyless spend
//! builders in the same boundary style as `payment.rs`. The XCH consolidation is ALSO proven on the
//! real Chia simulator (`chia-sdk-test`): N coins collapse into ONE self coin under real consensus.

use chia_puzzle_types::standard::StandardArgs;
use chia_puzzle_types::DeriveSynthetic;
use chia_sdk_driver::{Cat, CatInfo, SpendContext, StandardLayer};
use chia_sdk_test::Simulator;
use chia_sdk_types::Conditions;
use chip35_dl_coin::{
    build_cat_consolidation, build_coin_consolidation, master_to_wallet_unhardened, select_coins,
    spend_bundle_to_hex, Bytes32, Coin, CoinSelection, Error, PublicKey, SecretKey,
    SelectCoinsResult, Signature, SpendBundle, DEFAULT_COIN_CAP,
};

fn synthetic_for(seed: u8) -> PublicKey {
    let sk = SecretKey::from_seed(&[seed; 32]);
    master_to_wallet_unhardened(&sk.public_key(), 0).derive_synthetic()
}

fn owner_ph(synth: PublicKey) -> Bytes32 {
    StandardArgs::curry_tree_hash(synth).into()
}

/// A coin at `ph` of `amount`, with a distinct `parent` so its coin id (the tie-break key) is unique.
fn coin_p(parent: u8, ph: Bytes32, amount: u64) -> Coin {
    Coin {
        parent_coin_info: Bytes32::new([parent; 32]),
        puzzle_hash: ph,
        amount,
    }
}

fn hexed(css: Vec<chip35_dl_coin::CoinSpend>) -> String {
    spend_bundle_to_hex(&SpendBundle::new(css, Signature::default())).unwrap()
}

// ---------------------------------------------------------------------------
// select_coins — ordering + Ok result
// ---------------------------------------------------------------------------

#[test]
fn default_cap_is_50() {
    assert_eq!(DEFAULT_COIN_CAP, 50);
}

#[test]
fn selects_descending_largest_first() {
    let ph = owner_ph(synthetic_for(1));
    let coins = vec![
        coin_p(1, ph, 5),
        coin_p(2, ph, 30),
        coin_p(3, ph, 10),
        coin_p(4, ph, 20),
    ];
    // Target 45 → needs the two largest (30 + 20 = 50 >= 45) in descending order.
    match select_coins(coins, 45, 50) {
        SelectCoinsResult::Ok(CoinSelection {
            coins,
            total,
            change,
        }) => {
            assert_eq!(
                coins.iter().map(|c| c.amount).collect::<Vec<_>>(),
                vec![30, 20],
                "largest-first order"
            );
            assert_eq!(total, 50);
            assert_eq!(change, 5);
        }
        other => panic!("expected Ok, got {other:?}"),
    }
}

#[test]
fn single_large_coin_covers_target_with_change() {
    let ph = owner_ph(synthetic_for(1));
    let coins = vec![coin_p(1, ph, 100), coin_p(2, ph, 1)];
    match select_coins(coins, 40, 50) {
        SelectCoinsResult::Ok(sel) => {
            assert_eq!(sel.coins.len(), 1);
            assert_eq!(sel.coins[0].amount, 100);
            assert_eq!(sel.total, 100);
            assert_eq!(sel.change, 60);
        }
        other => panic!("expected Ok, got {other:?}"),
    }
}

#[test]
fn ties_break_by_coin_id_deterministically() {
    let ph = owner_ph(synthetic_for(1));
    // Two equal-amount coins; the tie-break is coin id ascending. Build both, take the two of value 7.
    let a = coin_p(0xAA, ph, 7);
    let b = coin_p(0xBB, ph, 7);
    let expected_first = if a.coin_id() <= b.coin_id() { a } else { b };
    let coins = vec![b, a]; // input order intentionally reversed
    match select_coins(coins, 14, 50) {
        SelectCoinsResult::Ok(sel) => {
            assert_eq!(sel.coins.len(), 2);
            assert_eq!(
                sel.coins[0].coin_id(),
                expected_first.coin_id(),
                "smaller coin id leads on an amount tie"
            );
        }
        other => panic!("expected Ok, got {other:?}"),
    }
}

#[test]
fn select_is_deterministic() {
    let ph = owner_ph(synthetic_for(1));
    let mk = || {
        vec![
            coin_p(1, ph, 5),
            coin_p(2, ph, 30),
            coin_p(3, ph, 10),
            coin_p(4, ph, 20),
        ]
    };
    let a = select_coins(mk(), 45, 50);
    let b = select_coins(mk(), 45, 50);
    assert_eq!(a, b, "identical inputs => identical selection");
}

#[test]
fn target_zero_selects_nothing() {
    let ph = owner_ph(synthetic_for(1));
    match select_coins(vec![coin_p(1, ph, 5)], 0, 50) {
        SelectCoinsResult::Ok(sel) => {
            assert!(sel.coins.is_empty());
            assert_eq!(sel.total, 0);
            assert_eq!(sel.change, 0);
        }
        other => panic!("expected Ok, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// select_coins — the cap boundary + the two distinct failure modes
// ---------------------------------------------------------------------------

#[test]
fn cap_exactly_enough_coins_is_ok() {
    let ph = owner_ph(synthetic_for(1));
    // cap 3, four coins of 10, target 30 → exactly 3 coins reach it → Ok.
    let coins = vec![
        coin_p(1, ph, 10),
        coin_p(2, ph, 10),
        coin_p(3, ph, 10),
        coin_p(4, ph, 10),
    ];
    match select_coins(coins, 30, 3) {
        SelectCoinsResult::Ok(sel) => {
            assert_eq!(sel.coins.len(), 3);
            assert_eq!(sel.total, 30);
            assert_eq!(sel.change, 0);
        }
        other => panic!("expected Ok at the cap boundary, got {other:?}"),
    }
}

#[test]
fn cap_one_too_many_needs_consolidation() {
    let ph = owner_ph(synthetic_for(1));
    // cap 3, four coins of 10, target 40 → needs all 4 (one over the cap) → NeedsConsolidation,
    // NOT insufficient (the value is present).
    let coins = vec![
        coin_p(1, ph, 10),
        coin_p(2, ph, 10),
        coin_p(3, ph, 10),
        coin_p(4, ph, 10),
    ];
    match select_coins(coins, 40, 3) {
        SelectCoinsResult::NeedsConsolidation {
            available_coin_count,
            available_total,
            required,
            cap,
        } => {
            assert_eq!(available_coin_count, 4);
            assert_eq!(available_total, 40);
            assert_eq!(required, 40);
            assert_eq!(cap, 3);
        }
        other => panic!("expected NeedsConsolidation, got {other:?}"),
    }
}

#[test]
fn insufficient_funds_is_distinct_from_needs_consolidation() {
    let ph = owner_ph(synthetic_for(1));
    // Total value (20) is below the target (30) → genuinely insufficient, regardless of cap.
    let coins = vec![coin_p(1, ph, 10), coin_p(2, ph, 10)];
    match select_coins(coins, 30, 50) {
        SelectCoinsResult::InsufficientFunds {
            available_coin_count,
            available_total,
            required,
            cap,
        } => {
            assert_eq!(available_coin_count, 2);
            assert_eq!(available_total, 20);
            assert_eq!(required, 30);
            assert_eq!(cap, 50);
        }
        other => panic!("expected InsufficientFunds, got {other:?}"),
    }
}

#[test]
fn empty_coins_is_insufficient() {
    match select_coins(vec![], 1, 50) {
        SelectCoinsResult::InsufficientFunds {
            available_coin_count,
            available_total,
            ..
        } => {
            assert_eq!(available_coin_count, 0);
            assert_eq!(available_total, 0);
        }
        other => panic!("expected InsufficientFunds, got {other:?}"),
    }
}

#[test]
fn many_small_coins_over_cap_needs_consolidation() {
    let ph = owner_ph(synthetic_for(1));
    // 60 coins of 1; target 60 needs all 60 > cap 50 → NeedsConsolidation.
    let coins: Vec<Coin> = (0..60).map(|i| coin_p(i as u8, ph, 1)).collect();
    assert!(matches!(
        select_coins(coins.clone(), 60, 50),
        SelectCoinsResult::NeedsConsolidation { .. }
    ));
    // A cap large enough to hold them all → Ok.
    assert!(matches!(
        select_coins(coins, 60, 100),
        SelectCoinsResult::Ok(_)
    ));
}

// ---------------------------------------------------------------------------
// build_coin_consolidation (XCH) — keyless structural + error paths
// ---------------------------------------------------------------------------

#[test]
fn consolidation_requires_two_coins() {
    let key = synthetic_for(2);
    let ph = owner_ph(key);
    assert!(matches!(
        build_coin_consolidation(key, vec![coin_p(1, ph, 10)], 50, 0),
        Err(Error::Parse(_))
    ));
    assert!(matches!(
        build_coin_consolidation(key, vec![], 50, 0),
        Err(Error::Parse(_))
    ));
}

#[test]
fn consolidation_cap_below_two_errors() {
    let key = synthetic_for(2);
    let ph = owner_ph(key);
    // A cap of 1 could never merge (needs >= 2) even with many coins available.
    assert!(matches!(
        build_coin_consolidation(key, vec![coin_p(1, ph, 10), coin_p(2, ph, 10)], 1, 0),
        Err(Error::Parse(_))
    ));
}

#[test]
fn consolidation_fee_not_below_amount_errors() {
    let key = synthetic_for(2);
    let ph = owner_ph(key);
    // Merged total 2 with a fee of 2 leaves nothing to consolidate.
    assert!(matches!(
        build_coin_consolidation(key, vec![coin_p(1, ph, 1), coin_p(2, ph, 1)], 50, 2),
        Err(Error::Parse(_))
    ));
}

#[test]
fn consolidation_merges_only_the_smallest_cap_coins() {
    let key = synthetic_for(2);
    let ph = owner_ph(key);
    // Five coins; cap 3 merges the three SMALLEST (1, 2, 3), leaving the two large ones untouched.
    let coins = vec![
        coin_p(1, ph, 100),
        coin_p(2, ph, 1),
        coin_p(3, ph, 50),
        coin_p(4, ph, 2),
        coin_p(5, ph, 3),
    ];
    let css = build_coin_consolidation(key, coins, 3, 0).expect("consolidation");
    let mut merged: Vec<u64> = css.iter().map(|cs| cs.coin.amount).collect();
    merged.sort_unstable();
    assert_eq!(merged, vec![1, 2, 3], "only the smallest 3 coins are spent");
}

#[test]
fn consolidation_is_deterministic() {
    let key = synthetic_for(2);
    let ph = owner_ph(key);
    let mk = || vec![coin_p(1, ph, 10), coin_p(2, ph, 20), coin_p(3, ph, 30)];
    let a = build_coin_consolidation(key, mk(), 50, 5).unwrap();
    let b = build_coin_consolidation(key, mk(), 50, 5).unwrap();
    assert_eq!(hexed(a), hexed(b), "identical inputs => identical bytes");
}

// ---------------------------------------------------------------------------
// build_cat_consolidation (CAT tail) — keyless structural + error paths
// ---------------------------------------------------------------------------

/// A keyless eve [`Cat`] of `amount` at the key's p2 puzzle hash, with a distinct parent coin id.
fn cat_of(key: PublicKey, asset_id: Bytes32, parent: u8, amount: u64) -> Cat {
    let p2 = owner_ph(key);
    Cat::new(
        Coin {
            parent_coin_info: Bytes32::new([parent; 32]),
            puzzle_hash: Bytes32::new([0x77; 32]),
            amount,
        },
        None,
        CatInfo::new(asset_id, None, p2),
    )
}

#[test]
fn cat_consolidation_requires_two() {
    let key = synthetic_for(3);
    let asset = Bytes32::new([0xAB; 32]);
    assert!(matches!(
        build_cat_consolidation(key, vec![cat_of(key, asset, 1, 10)], 50),
        Err(Error::Parse(_))
    ));
}

#[test]
fn cat_consolidation_rejects_mixed_asset_ids() {
    let key = synthetic_for(3);
    let a = cat_of(key, Bytes32::new([1; 32]), 1, 10);
    let b = cat_of(key, Bytes32::new([2; 32]), 2, 10);
    assert!(matches!(
        build_cat_consolidation(key, vec![a, b], 50),
        Err(Error::Parse(_))
    ));
}

#[test]
fn cat_consolidation_cap_below_two_errors() {
    let key = synthetic_for(3);
    let asset = Bytes32::new([0xAB; 32]);
    assert!(matches!(
        build_cat_consolidation(
            key,
            vec![cat_of(key, asset, 1, 10), cat_of(key, asset, 2, 10)],
            1
        ),
        Err(Error::Parse(_))
    ));
}

#[test]
fn cat_consolidation_merges_smallest_cap_and_is_deterministic() {
    let key = synthetic_for(3);
    let asset = Bytes32::new([0xAB; 32]);
    let mk = || {
        vec![
            cat_of(key, asset, 1, 100),
            cat_of(key, asset, 2, 1),
            cat_of(key, asset, 3, 50),
            cat_of(key, asset, 4, 2),
        ]
    };
    let css = build_cat_consolidation(key, mk(), 3).expect("cat consolidation");
    assert!(!css.is_empty(), "produces coin spends");
    // Determinism (byte-identical bundle for identical inputs).
    let a = build_cat_consolidation(key, mk(), 3).unwrap();
    let b = build_cat_consolidation(key, mk(), 3).unwrap();
    assert_eq!(hexed(a), hexed(b));
}

// ---------------------------------------------------------------------------
// SIMULATOR VALIDATION GATE — XCH consolidation collapses N coins into ONE under real consensus.
// ---------------------------------------------------------------------------

/// The #413 proof: several XCH coins at one address CONSOLIDATE into a SINGLE self coin (minus fee)
/// that VALIDATES on the in-process Chia simulator. Proves the merge bundle balances under real
/// consensus and the one output coin actually exists on-chain afterwards.
#[test]
fn xch_consolidation_validates_on_simulator() -> anyhow::Result<()> {
    let mut sim = Simulator::new();
    let alice = sim.bls(100);
    let alice_p2 = StandardLayer::new(alice.pk);

    // (tx1) Split the single 100-mojo coin into several coins at Alice's OWN address (5, 10, 20) plus
    // 65 change — a realistic fragmented wallet.
    let ctx = &mut SpendContext::new();
    let hint = ctx.hint(alice.puzzle_hash)?;
    let split = Conditions::new()
        .create_coin(alice.puzzle_hash, 5, hint)
        .create_coin(alice.puzzle_hash, 10, hint)
        .create_coin(alice.puzzle_hash, 20, hint)
        .create_coin(alice.puzzle_hash, 65, hint);
    alice_p2.spend(ctx, alice.coin, split)?;
    sim.spend_coins(ctx.take(), std::slice::from_ref(&alice.sk))?;

    // The three fragments now on-chain (coin id = sha256(parent || puzzle_hash || amount)).
    let frag = |amount: u64| Coin {
        parent_coin_info: alice.coin.coin_id(),
        puzzle_hash: alice.puzzle_hash,
        amount,
    };
    let (c5, c10, c20) = (frag(5), frag(10), frag(20));
    assert!(sim.coin_state(c5.coin_id()).is_some(), "fragment exists");

    // (tx2) Consolidate the three fragments (cap 50, fee 2) → ONE 33-mojo self coin.
    let css = build_coin_consolidation(alice.pk, vec![c20, c5, c10], 50, 2)?;
    sim.spend_coins(css, std::slice::from_ref(&alice.sk))
        .map_err(|e| anyhow::anyhow!("consolidation spend failed: {e:?}"))?;

    // The lead coin (smallest = c5) is the parent of the single consolidated output (35 - 2 fee = 33).
    let consolidated = Coin {
        parent_coin_info: c5.coin_id(),
        puzzle_hash: alice.puzzle_hash,
        amount: 33,
    };
    assert!(
        sim.coin_state(consolidated.coin_id()).is_some(),
        "the single consolidated coin (33 mojo) exists on-chain"
    );
    // The merged fragments are gone (spent).
    assert!(
        sim.coin_state(c20.coin_id())
            .is_some_and(|s| s.spent_height.is_some()),
        "merged fragment c20 was spent"
    );
    Ok(())
}
