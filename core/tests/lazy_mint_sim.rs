//! SIMULATOR VALIDATION GATE for the trustless lazy mint (#40).
//!
//! This is how we TRUST the primitive: it runs the commit + claim spends on a real simulated Chia
//! chain (`chia-sdk-test`), not a keyless shape check. It proves the end-to-end mint-on-claim flow:
//!   (a) fund a coin + create a DID,
//!   (b) `build_lazy_mint_commit` for a 3-item collection and push it,
//!   (c) `build_lazy_mint_claim` for ONE item AS A DIFFERENT PARTY and push it,
//!   (d) the resulting NFT exists, is unspent, is owned by the claimer, and its launcher lineage
//!       traces back to the creator DID (provenance by lineage — see DESIGN.md #40).

use chia_protocol::Bytes32;
use chia_puzzle_types::standard::StandardArgs;
use chia_sdk_driver::{Launcher, Nft, Puzzle, SpendContext, StandardLayer};
use chia_sdk_test::Simulator;
use chip35_dl_coin::{
    build_lazy_mint_claim, build_lazy_mint_commit, sha256, Collection, LazyMintItem,
    LazyMintPolicy, NftMediaMetadata,
};
use clvm_traits::ToClvm;
use clvmr::Allocator;

fn item(i: usize) -> LazyMintItem {
    LazyMintItem {
        metadata: NftMediaMetadata {
            data_uris: vec![format!("dig://urn:dig:chia:store:root/item{i}.png")],
            data_hash: Some(sha256(format!("bytes-{i}").as_bytes())),
            metadata_uris: vec![format!("dig://urn:dig:chia:store:root/item{i}.json")],
            metadata_hash: Some(sha256(format!("meta-{i}").as_bytes())),
            license_uris: vec![],
            license_hash: None,
            edition_number: 1,
            edition_total: 1,
        },
        royalty_basis_points: 300,
    }
}

#[test]
fn lazy_mint_commit_then_claim_as_different_party() -> anyhow::Result<()> {
    let mut sim = Simulator::new();

    // (a) fund the CREATOR with a coin, and a separate CLAIMER with their own coin.
    let creator = sim.bls(10);
    let creator_p2 = StandardLayer::new(creator.pk);
    let claimer = sim.bls(10);
    let claimer_ph: Bytes32 = StandardArgs::curry_tree_hash(claimer.pk).into();

    // (a) create a DID for the creator (the single authority that precommits the collection).
    let ctx = &mut SpendContext::new();
    let (create_did, did) =
        Launcher::new(creator.coin.coin_id(), 1).create_simple_did(ctx, &creator_p2)?;
    creator_p2.spend(ctx, creator.coin, create_did)?;
    sim.spend_coins(ctx.take(), std::slice::from_ref(&creator.sk))?;
    let did_coin_id = did.coin.coin_id();

    // (b) the creator DID spends ONCE to precommit a 3-item collection.
    let collection = Collection {
        id: "lazy-col".into(),
        name: "DIG Lazy Punks".into(),
        attributes: vec![],
        royalty_puzzle_hash: creator.puzzle_hash,
        royalty_basis_points: 300,
    };
    let items = vec![item(0), item(1), item(2)];
    let commit = build_lazy_mint_commit(
        creator.pk,
        did,
        &collection,
        &items,
        LazyMintPolicy::DirectMint,
        None,
    )?;
    sim.spend_coins(
        commit.coin_spends.clone(),
        std::slice::from_ref(&creator.sk),
    )
    .map_err(|e| anyhow::anyhow!("COMMIT spend failed: {e:?}"))?;

    // Every precommitted commitment coin now exists on-chain (created by the DID spend).
    for cc in &commit.commit_coins {
        assert!(
            sim.coin_state(cc.coin_id()).is_some(),
            "commitment coin exists after commit"
        );
        assert_eq!(
            cc.parent_coin_info, did_coin_id,
            "commitment coin's parent is the DID coin"
        );
    }

    // (c) a DIFFERENT party (the claimer) unrolls + mints item 1, funding the mojo from THEIR coin.
    let descriptor = commit.descriptor();
    let claim = build_lazy_mint_claim(
        claimer.pk,
        vec![claimer.coin],
        claimer_ph,
        &descriptor,
        1,
        None,
        0,
    )?;
    assert_eq!(
        claim.launcher_id, commit.launcher_ids[1],
        "claim mints exactly the precommitted launcher id"
    );
    // Signed only by the CLAIMER — no creator key, proving no further DID involvement.
    sim.spend_coins(claim.coin_spends.clone(), std::slice::from_ref(&claimer.sk))
        .map_err(|e| anyhow::anyhow!("CLAIM spend failed: {e:?}"))?;

    // (d) the minted NFT exists and is UNSPENT.
    let nft_state = sim
        .coin_state(claim.nft_coin.coin_id())
        .expect("minted NFT coin exists");
    assert!(
        nft_state.spent_height.is_none(),
        "minted NFT is live (unspent)"
    );

    // (d) the NFT is owned by the CLAIMER. Re-parse the NFT from its parent (the eve) spend.
    let nft_parent = claim.nft_coin.parent_coin_info;
    let parent_puzzle_reveal = sim
        .puzzle_reveal(nft_parent)
        .expect("eve NFT puzzle reveal present");
    let parent_solution = sim.solution(nft_parent).expect("eve NFT solution present");
    let parent_coin = sim.coin_state(nft_parent).expect("eve NFT coin state").coin;

    let mut allocator = Allocator::new();
    let puzzle_ptr = parent_puzzle_reveal.to_clvm(&mut allocator)?;
    let solution_ptr = parent_solution.to_clvm(&mut allocator)?;
    let puzzle = Puzzle::parse(&allocator, puzzle_ptr);
    let nft = Nft::parse_child(&mut allocator, parent_coin, puzzle, solution_ptr)?
        .expect("parsed an NFT from the eve spend");

    assert_eq!(
        nft.info.p2_puzzle_hash, claimer_ph,
        "the minted NFT is owned by the claimer's puzzle hash"
    );
    assert_eq!(
        nft.info.launcher_id, commit.launcher_ids[1],
        "the minted NFT's launcher id is the precommitted one"
    );

    // (d) PROVENANCE BY LINEAGE: the NFT's launcher coin descends from the creator's commitment coin,
    // which descends from the creator DID coin — verifiable creator attribution without a DID re-spend.
    let launcher_coin = sim
        .coin_state(commit.launcher_ids[1])
        .expect("launcher coin exists")
        .coin;
    let commit_coin_id = commit.commit_coins[1].coin_id();
    assert_eq!(
        launcher_coin.parent_coin_info, commit_coin_id,
        "the NFT launcher descends from the item's commitment coin"
    );
    assert_eq!(
        commit.commit_coins[1].parent_coin_info, did_coin_id,
        "the commitment coin descends from the creator DID coin (provenance by lineage)"
    );

    Ok(())
}
