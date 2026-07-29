//! SIMULATOR VALIDATION GATE for the NON-DID funded multi-item (N>=1) collection bulk mint (#1132).
//!
//! The DID-gated [`build_bulk_mint_funded`] mints N NFTs authorized by a single DID spend. The hub's
//! non-DID bulk mint has no DID coin: it funds N singleton launchers from a COIN SET with a SINGLE
//! network fee. [`build_bulk_mint_funded_no_did`] roots every item's `IntermediateLauncher` at the
//! lead funding coin, so ONE spend mints all N NFTs, funds `items.len()` launcher mojos + the fee
//! bundle-wide, and returns change.
//!
//! These tests are the #1132 proof and the #304 no-double-spend regression: they run on a real
//! simulated Chia chain (`chia-sdk-test`) and assert the funding math, that each selected coin is
//! spent EXACTLY once (the naive "loop a single-mint builder over ONE coin" approach double-spends
//! that coin and fails consensus), the change comes back, and the underfunded case errors cleanly.

use std::collections::HashSet;

use chia_puzzle_types::standard::StandardArgs;
use chia_sdk_test::Simulator;
use chip35_dl_coin::{
    build_bulk_mint_funded_no_did, sha256, Bytes32, Coin, Collection, CollectionAttribute, Error,
    ManifestItem, ManifestMedia,
};

fn collection(royalty_ph: Bytes32) -> Collection {
    Collection {
        id: "dig-punks".into(),
        name: "DIG Punks".into(),
        attributes: vec![CollectionAttribute {
            kind: "website".into(),
            value: "https://dig.net".into(),
        }],
        royalty_puzzle_hash: royalty_ph,
        royalty_basis_points: 300,
    }
}

/// `n` distinct manifest items (each with a unique `data_hash`) — enough to prove an arbitrary N.
fn items_n(n: usize) -> Vec<ManifestItem> {
    (0..n)
        .map(|i| ManifestItem {
            name: format!("DIG Punk #{i}"),
            description: None,
            attributes: vec![],
            media: ManifestMedia {
                data_uris: vec![format!("urn:dig:chia:store:root/item{i}.png")],
                data_hash: Some(sha256(format!("bytes-{i}").as_bytes())),
                ..Default::default()
            },
        })
        .collect()
}

/// Underfunded (coin-set total < `items.len()` + fee) errors cleanly BEFORE building any spend, with a
/// clear, actionable message.
#[test]
fn build_bulk_mint_funded_no_did_rejects_underfunded_coin_set() {
    let mut sim = Simulator::new();
    let minter = sim.bls(2);
    let three = items_n(3);
    // Total 2 mojo can't cover 3 launchers + fee 1 (needs >= 4).
    let err = build_bulk_mint_funded_no_did(
        minter.pk,
        &collection(minter.puzzle_hash),
        &three,
        minter.puzzle_hash,
        vec![minter.coin],
        1,
    )
    .unwrap_err();
    assert!(
        matches!(&err, Error::Parse(m) if m.contains("needs at least 4 mojo")),
        "got: {err}"
    );
}

/// THE #1132 + #304 proof: a MULTI-item (N=3) NON-DID collection mint funded by a COIN SET with a
/// single fee VALIDATES on the in-process Chia simulator, and each selected coin is spent EXACTLY
/// once. The naive hub approach (loop a single-mint builder over ONE coin N times) would spend that
/// coin N times — a double-spend that fails consensus; this test would FAIL against it.
#[test]
fn build_bulk_mint_funded_no_did_validates_on_simulator_no_double_spend() -> anyhow::Result<()> {
    let mut sim = Simulator::new();

    // A coin SET (two coins, 8 + 4 = 12) funds 3 launchers (3 mojo) + fee (2 mojo) = 5 needed → 7 change.
    let minter = sim.bls(12);
    let minter_second = Coin {
        parent_coin_info: minter.coin.coin_id(),
        puzzle_hash: minter.puzzle_hash,
        amount: 8,
    };
    // Materialise the second coin on-chain by paying it out of the first.
    {
        use chia_sdk_driver::{SpendContext, StandardLayer};
        use chia_sdk_types::Conditions;
        let ctx = &mut SpendContext::new();
        let p2 = StandardLayer::new(minter.pk);
        p2.spend(
            ctx,
            minter.coin,
            Conditions::new()
                .create_coin(minter.puzzle_hash, 8, chia_puzzle_types::Memos::None)
                .create_coin(minter.puzzle_hash, 4, chia_puzzle_types::Memos::None),
        )?;
        sim.spend_coins(ctx.take(), std::slice::from_ref(&minter.sk))?;
    }
    let coin_a = minter_second; // 8
    let coin_b = Coin {
        parent_coin_info: minter.coin.coin_id(),
        puzzle_hash: minter.puzzle_hash,
        amount: 4,
    };
    assert!(sim.coin_state(coin_a.coin_id()).is_some());
    assert!(sim.coin_state(coin_b.coin_id()).is_some());

    let recipient: Bytes32 = StandardArgs::curry_tree_hash(minter.pk).into();
    let three = items_n(3);
    let fee = 2u64;

    let resp = build_bulk_mint_funded_no_did(
        minter.pk,
        &collection(minter.puzzle_hash),
        &three,
        recipient,
        vec![coin_a, coin_b],
        fee,
    )?;
    assert_eq!(resp.launcher_ids.len(), 3, "three NFTs produced");
    let unique: HashSet<_> = resp.launcher_ids.iter().collect();
    assert_eq!(unique.len(), 3, "launcher ids are distinct");

    // NO DOUBLE-SPEND: every selected funding coin is spent EXACTLY once across the bundle.
    let spent: Vec<Bytes32> = resp
        .coin_spends
        .iter()
        .map(|cs| cs.coin.coin_id())
        .collect();
    let spent_set: HashSet<_> = spent.iter().collect();
    assert_eq!(
        spent.len(),
        spent_set.len(),
        "no coin is spent more than once in the bundle"
    );
    assert!(spent_set.contains(&coin_a.coin_id()), "coin_a is spent");
    assert!(spent_set.contains(&coin_b.coin_id()), "coin_b is spent");

    sim.spend_coins(resp.coin_spends, std::slice::from_ref(&minter.sk))
        .map_err(|e| anyhow::anyhow!("no-did funded bulk-mint spend failed: {e:?}"))?;

    // Change (12 total − 3 launchers − 2 fee = 7) landed back at the minter's own address.
    let change = Coin {
        parent_coin_info: coin_a.coin_id(),
        puzzle_hash: minter.puzzle_hash,
        amount: 7,
    };
    assert!(
        sim.coin_state(change.coin_id()).is_some(),
        "7-mojo change coin should exist"
    );
    Ok(())
}

/// N=1 (single item) with fee=0 and EXACT funding (1 mojo for the 1 launcher) validates with no change.
#[test]
fn build_bulk_mint_funded_no_did_single_item_exact_no_change() -> anyhow::Result<()> {
    let mut sim = Simulator::new();
    let minter = sim.bls(1);
    let one = items_n(1);

    let resp = build_bulk_mint_funded_no_did(
        minter.pk,
        &collection(minter.puzzle_hash),
        &one,
        minter.puzzle_hash,
        vec![minter.coin],
        0,
    )?;
    assert_eq!(resp.launcher_ids.len(), 1, "one NFT produced");

    sim.spend_coins(resp.coin_spends, std::slice::from_ref(&minter.sk))
        .map_err(|e| anyhow::anyhow!("single-item no-did mint spend failed: {e:?}"))?;

    // The 1-mojo coin was fully consumed by the single launcher — no change coin.
    let would_be_change = Coin {
        parent_coin_info: minter.coin.coin_id(),
        puzzle_hash: minter.puzzle_hash,
        amount: 0,
    };
    assert!(
        sim.coin_state(would_be_change.coin_id()).is_none(),
        "no change coin for an exactly-sized single-item mint"
    );
    Ok(())
}

/// Empty items and empty coin set both error cleanly.
#[test]
fn build_bulk_mint_funded_no_did_rejects_empty_inputs() {
    let mut sim = Simulator::new();
    let minter = sim.bls(5);

    let empty_items = build_bulk_mint_funded_no_did(
        minter.pk,
        &collection(minter.puzzle_hash),
        &[],
        minter.puzzle_hash,
        vec![minter.coin],
        0,
    )
    .unwrap_err();
    assert!(matches!(&empty_items, Error::Parse(m) if m.contains("items is empty")));

    let empty_coins = build_bulk_mint_funded_no_did(
        minter.pk,
        &collection(minter.puzzle_hash),
        &items_n(1),
        minter.puzzle_hash,
        vec![],
        0,
    )
    .unwrap_err();
    assert!(matches!(&empty_coins, Error::Parse(m) if m.contains("selected_coins is empty")));
}

/// Funding arithmetic overflow is detected and returns a clean error, never a panic or miscomputed bundle.
#[test]
fn funding_arithmetic_overflow_is_a_clean_error() {
    let mut sim = Simulator::new();
    let minter = sim.bls(u64::MAX);
    let one = items_n(1);

    // fee = u64::MAX causes launcher_mojos (1) + fee to overflow during checked_add.
    let err = build_bulk_mint_funded_no_did(
        minter.pk,
        &collection(minter.puzzle_hash),
        &one,
        minter.puzzle_hash,
        vec![minter.coin],
        u64::MAX,
    )
    .unwrap_err();
    assert!(
        matches!(&err, Error::Parse(m) if m.contains("overflows u64")),
        "expected overflow error, got: {err}"
    );
}
