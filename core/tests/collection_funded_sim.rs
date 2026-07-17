//! SIMULATOR VALIDATION GATE for the FUNDED multi-item (N>1) collection bulk mint (#221).
//!
//! `build_bulk_mint` is structurally correct for any `items.len()`, but a REAL on-chain mint of N>1
//! items needs more value than the DID singleton carries: each item's `IntermediateLauncher` prints
//! 1 mojo (a 0-value intermediate coin's own spend creates a 1-mojo launcher coin) that must be
//! donated by another coin in the SAME bundle — and the DID's `update` spend conserves its own value
//! exactly, so it cannot fund the extra launchers. `build_bulk_mint_funded` spends a separate XCH
//! `funding_coin` contributing exactly `items.len()` mojos, returning any excess as change.
//!
//! This is the chip35 twin of digstore's `build_collection_mint_funded_in_validates_on_simulator`
//! (#199): it runs the funded N=3 mint on a real simulated Chia chain (`chia-sdk-test`) — proving the
//! bundle balances under real consensus and the change comes back rather than being burned as fee.

use chia_puzzle_types::standard::StandardArgs;
use chia_sdk_driver::{Launcher, SpendContext, StandardLayer};
use chia_sdk_test::Simulator;
use chip35_dl_coin::{
    build_bulk_mint_funded, sha256, Bytes32, Coin, Collection, CollectionAttribute, Error,
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

/// `n` distinct manifest items (each with a unique `data_hash`) — enough to prove an arbitrary N>1.
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

/// [`build_bulk_mint_funded`] refuses a funding coin too small to cover the per-item launcher mojos
/// (1 mojo per item), with a clear, actionable message — BEFORE building any spend.
#[test]
fn build_bulk_mint_funded_rejects_underfunded_coin() {
    let mut sim = Simulator::new();
    let alice = sim.bls(1);
    let alice_p2 = StandardLayer::new(alice.pk);
    let ctx = &mut SpendContext::new();
    let (_create_did, did) = Launcher::new(alice.coin.coin_id(), 1)
        .create_simple_did(ctx, &alice_p2)
        .expect("create did");

    let three = items_n(3);
    // A funding coin worth only 2 mojo can't cover 3 items (needs >= 3).
    let underfunded = Coin {
        parent_coin_info: Bytes32::new([0x99; 32]),
        puzzle_hash: alice.puzzle_hash,
        amount: 2,
    };
    let err = build_bulk_mint_funded(
        alice.pk,
        did,
        &collection(alice.puzzle_hash),
        &three,
        alice.puzzle_hash,
        underfunded,
        alice.pk,
    )
    .unwrap_err();
    assert!(
        matches!(&err, Error::Parse(m) if m.contains("needs at least 3 mojo")),
        "got: {err}"
    );
}

/// THE #221 proof: a MULTI-item (N=3) DID-attributed collection mint, funded by a separate XCH coin,
/// VALIDATES on the in-process Chia simulator — every NFT mints, all attributed to the same DID, and
/// the whole bundle (DID spend + funding-coin spend + 3 intermediate launchers) balances under real
/// consensus. This is the proof the unfunded `build_bulk_mint` lacks for N>1: it builds the spends
/// structurally, but they fail consensus for real value-conservation reasons.
///
/// Two transactions mirror the real hub flow: (tx1) the creator DID is committed on-chain, then
/// (tx2) the funded bulk mint spends that on-chain DID + a separate funding coin. The 7-mojo change
/// (10 funded − 3 needed) must land back at the funder's own address, proving the excess is returned
/// as change rather than silently burned as network fee.
#[test]
fn build_bulk_mint_funded_validates_on_simulator() -> anyhow::Result<()> {
    let mut sim = Simulator::new();

    // (tx1) Create the creator DID on-chain. A 1-mojo coin funds the 1-mojo singleton exactly.
    let creator = sim.bls(1);
    let creator_p2 = StandardLayer::new(creator.pk);
    let ctx = &mut SpendContext::new();
    let (create_did, did) =
        Launcher::new(creator.coin.coin_id(), 1).create_simple_did(ctx, &creator_p2)?;
    creator_p2.spend(ctx, creator.coin, create_did)?;
    sim.spend_coins(ctx.take(), std::slice::from_ref(&creator.sk))?;
    let did_launcher = did.info.launcher_id;

    // (tx2) A SEPARATE XCH coin funds the 3 mojo the intermediate-launcher trick needs. Fund 10 (more
    // than 3) to also prove change comes back rather than being burned as fee.
    let funder = sim.bls(10);
    let recipient: Bytes32 = StandardArgs::curry_tree_hash(creator.pk).into();
    let three = items_n(3);

    let resp = build_bulk_mint_funded(
        creator.pk,
        did,
        &collection(creator.puzzle_hash),
        &three,
        recipient,
        funder.coin,
        funder.pk,
    )?;
    assert_eq!(resp.launcher_ids.len(), 3, "three NFTs produced");
    let unique: std::collections::HashSet<_> = resp.launcher_ids.iter().collect();
    assert_eq!(unique.len(), 3, "launcher ids are distinct");

    // Apply the whole bundle; consensus validates the value conservation + every mint. Sign with BOTH
    // the DID key (creator) and the funding-coin key (funder).
    sim.spend_coins(resp.coin_spends, &[creator.sk.clone(), funder.sk.clone()])
        .map_err(|e| anyhow::anyhow!("funded bulk-mint spend failed: {e:?}"))?;

    // The 7-mojo change (10 funded − 3 needed) landed back at the funder's own address — proving the
    // excess was NOT silently burned as network fee.
    let change = Coin {
        parent_coin_info: funder.coin.coin_id(),
        puzzle_hash: funder.puzzle_hash,
        amount: 7,
    };
    assert!(
        sim.coin_state(change.coin_id()).is_some(),
        "7-mojo change coin should exist"
    );

    let _ = did_launcher;
    Ok(())
}

/// EXACT funding (funding_coin.amount == items.len()) also VALIDATES on the simulator: the funding
/// coin is fully consumed by the launchers with NO change coin created (the `change == 0` path).
/// Complements the change-returning case above and proves a precisely-sized coin isn't rejected.
#[test]
fn build_bulk_mint_funded_exact_amount_no_change() -> anyhow::Result<()> {
    let mut sim = Simulator::new();

    let creator = sim.bls(1);
    let creator_p2 = StandardLayer::new(creator.pk);
    let ctx = &mut SpendContext::new();
    let (create_did, did) =
        Launcher::new(creator.coin.coin_id(), 1).create_simple_did(ctx, &creator_p2)?;
    creator_p2.spend(ctx, creator.coin, create_did)?;
    sim.spend_coins(ctx.take(), std::slice::from_ref(&creator.sk))?;

    // A funding coin worth EXACTLY 3 mojo for a 3-item mint — fully consumed, no change.
    let funder = sim.bls(3);
    let three = items_n(3);

    let resp = build_bulk_mint_funded(
        creator.pk,
        did,
        &collection(creator.puzzle_hash),
        &three,
        creator.puzzle_hash,
        funder.coin,
        funder.pk,
    )?;
    assert_eq!(resp.launcher_ids.len(), 3, "three NFTs produced");

    sim.spend_coins(resp.coin_spends, &[creator.sk.clone(), funder.sk.clone()])
        .map_err(|e| anyhow::anyhow!("exact-funded bulk-mint spend failed: {e:?}"))?;

    // No change coin exists at the funder's address (the whole coin funded the launchers).
    let would_be_change = Coin {
        parent_coin_info: funder.coin.coin_id(),
        puzzle_hash: funder.puzzle_hash,
        amount: 0,
    };
    assert!(
        sim.coin_state(would_be_change.coin_id()).is_none(),
        "no change coin for an exactly-sized funding coin"
    );
    Ok(())
}
