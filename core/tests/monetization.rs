//! Tests for the in-dapp monetization primitives (roadmap #46): payment (XCH + CAT), paywall
//! (pay-to-unlock verify), NFT-gating (prove ownership / collection membership), and the
//! subscription scaffold. Keyless-boundary style (no simulator) like `assets.rs` / `builders.rs`.

use chia_puzzle_types::{standard::StandardArgs, DeriveSynthetic};
use chia_sdk_driver::{Cat, CatInfo, SpendContext};
use chip35_dl_coin::{
    build_cat_payment, build_subscription_authorization, build_subscription_claim,
    build_xch_payment, create_did, master_to_wallet_unhardened, mint_nft, payment_nonce,
    prove_collection_membership, prove_nft_ownership, read_nft_ownership, spend_bundle_to_hex,
    verify_payment_receipt, Bytes32, Coin, DidAttribution, Error, GatingError, NftMediaMetadata,
    NftMintParams, ObservedPayment, PaymentAsset, PaywallError, PublicKey, SecretKey, Signature,
    SpendBundle, SubscriptionTerms,
};

fn synthetic_for(seed: u8) -> PublicKey {
    let sk = SecretKey::from_seed(&[seed; 32]);
    master_to_wallet_unhardened(&sk.public_key(), 0).derive_synthetic()
}

fn owner_ph(synth: PublicKey) -> Bytes32 {
    StandardArgs::curry_tree_hash(synth).into()
}

fn coin(ph: Bytes32, amount: u64) -> Coin {
    Coin {
        parent_coin_info: Bytes32::new([7u8; 32]),
        puzzle_hash: ph,
        amount,
    }
}

// ---------------------------------------------------------------------------
// Payment — XCH
// ---------------------------------------------------------------------------

#[test]
fn xch_payment_produces_spends_and_receipt() {
    let buyer = synthetic_for(2);
    let buyer_ph = owner_ph(buyer);
    let owner = owner_ph(synthetic_for(9)); // the dapp owner's address
    let nonce = payment_nonce(b"dapp:article-42:user:alice");

    let r = build_xch_payment(buyer, vec![coin(buyer_ph, 1000)], owner, 250, nonce, 10)
        .expect("build_xch_payment");

    assert!(!r.coin_spends.is_empty());
    assert_eq!(r.receipt.owner_puzzle_hash, owner);
    assert_eq!(r.receipt.amount, 250);
    assert_eq!(r.receipt.asset, PaymentAsset::Xch);
    assert_eq!(r.receipt.nonce, nonce);
    assert_eq!(r.receipt.payment_coin.puzzle_hash, owner);
    assert_eq!(r.receipt.payment_coin.amount, 250);
}

#[test]
fn xch_payment_is_deterministic() {
    let buyer = synthetic_for(2);
    let buyer_ph = owner_ph(buyer);
    let owner = owner_ph(synthetic_for(9));
    let nonce = payment_nonce(b"req-1");

    let a = build_xch_payment(buyer, vec![coin(buyer_ph, 1000)], owner, 250, nonce, 10).unwrap();
    let b = build_xch_payment(buyer, vec![coin(buyer_ph, 1000)], owner, 250, nonce, 10).unwrap();
    let ha = spend_bundle_to_hex(&SpendBundle::new(a.coin_spends, Signature::default())).unwrap();
    let hb = spend_bundle_to_hex(&SpendBundle::new(b.coin_spends, Signature::default())).unwrap();
    assert_eq!(ha, hb, "identical inputs => identical bytes");
}

#[test]
fn xch_payment_insufficient_funds_errors() {
    let buyer = synthetic_for(2);
    let buyer_ph = owner_ph(buyer);
    let owner = owner_ph(synthetic_for(9));
    let nonce = payment_nonce(b"req");
    // amount 250 + fee 10 > coin of 100
    assert!(matches!(
        build_xch_payment(buyer, vec![coin(buyer_ph, 100)], owner, 250, nonce, 10),
        Err(Error::Parse(_))
    ));
}

#[test]
fn xch_payment_no_coins_errors() {
    let buyer = synthetic_for(2);
    let owner = owner_ph(synthetic_for(9));
    assert!(matches!(
        build_xch_payment(buyer, vec![], owner, 1, payment_nonce(b"r"), 0),
        Err(Error::Parse(_))
    ));
}

// ---------------------------------------------------------------------------
// Payment — CAT (incl. DIG)
// ---------------------------------------------------------------------------

/// Build a buyer-owned eve [`Cat`] of the given amount for the test (keyless — we only need a
/// well-formed Cat to feed `build_cat_payment`; on-chain the caller parses real CATs).
fn buyer_cat(buyer: PublicKey, asset_id: Bytes32, amount: u64) -> Cat {
    let p2 = owner_ph(buyer);
    Cat::new(
        Coin {
            parent_coin_info: Bytes32::new([5u8; 32]),
            puzzle_hash: Bytes32::new([6u8; 32]),
            amount,
        },
        None,
        CatInfo::new(asset_id, None, p2),
    )
}

#[test]
fn cat_payment_produces_spends_and_receipt() {
    let buyer = synthetic_for(3);
    let owner = owner_ph(synthetic_for(9));
    let asset_id = Bytes32::new([0xABu8; 32]); // e.g. the DIG TAIL
    let nonce = payment_nonce(b"dapp:premium:user:bob");

    let cat = buyer_cat(buyer, asset_id, 1000);
    let r = build_cat_payment(buyer, vec![cat], owner, 600, nonce).expect("build_cat_payment");

    assert!(!r.coin_spends.is_empty());
    assert_eq!(r.receipt.asset, PaymentAsset::Cat(asset_id));
    assert_eq!(r.receipt.amount, 600);
    assert_eq!(r.receipt.owner_puzzle_hash, owner);
    assert_eq!(r.receipt.nonce, nonce);
}

#[test]
fn cat_payment_insufficient_errors() {
    let buyer = synthetic_for(3);
    let owner = owner_ph(synthetic_for(9));
    let asset_id = Bytes32::new([0xABu8; 32]);
    let cat = buyer_cat(buyer, asset_id, 100);
    assert!(matches!(
        build_cat_payment(buyer, vec![cat], owner, 600, payment_nonce(b"r")),
        Err(Error::Parse(_))
    ));
}

#[test]
fn cat_payment_mixed_asset_ids_error() {
    let buyer = synthetic_for(3);
    let owner = owner_ph(synthetic_for(9));
    let a = buyer_cat(buyer, Bytes32::new([1u8; 32]), 500);
    let b = buyer_cat(buyer, Bytes32::new([2u8; 32]), 500);
    assert!(matches!(
        build_cat_payment(buyer, vec![a, b], owner, 600, payment_nonce(b"r")),
        Err(Error::Parse(_))
    ));
}

#[test]
fn cat_payment_no_cats_errors() {
    let buyer = synthetic_for(3);
    let owner = owner_ph(synthetic_for(9));
    assert!(matches!(
        build_cat_payment(buyer, vec![], owner, 1, payment_nonce(b"r")),
        Err(Error::Parse(_))
    ));
}

// ---------------------------------------------------------------------------
// Paywall — verify_payment_receipt (pay-to-unlock)
// ---------------------------------------------------------------------------

fn observed(
    owner: Bytes32,
    amount: u64,
    asset: PaymentAsset,
    nonce: Option<Bytes32>,
) -> ObservedPayment {
    ObservedPayment {
        paid_to_puzzle_hash: owner,
        amount,
        asset,
        nonce,
    }
}

#[test]
fn paywall_grants_when_payment_matches() {
    let owner = owner_ph(synthetic_for(9));
    let nonce = payment_nonce(b"unlock-1");
    let obs = observed(owner, 250, PaymentAsset::Xch, Some(nonce));
    assert!(verify_payment_receipt(&obs, owner, 250, PaymentAsset::Xch, Some(nonce)).is_ok());
    // Overpayment still unlocks when min is met.
    let obs2 = observed(owner, 300, PaymentAsset::Xch, Some(nonce));
    assert!(verify_payment_receipt(&obs2, owner, 250, PaymentAsset::Xch, Some(nonce)).is_ok());
}

#[test]
fn paywall_denies_wrong_recipient() {
    let owner = owner_ph(synthetic_for(9));
    let other = owner_ph(synthetic_for(8));
    let nonce = payment_nonce(b"u");
    let obs = observed(other, 250, PaymentAsset::Xch, Some(nonce));
    assert!(matches!(
        verify_payment_receipt(&obs, owner, 250, PaymentAsset::Xch, Some(nonce)),
        Err(PaywallError::WrongRecipient { .. })
    ));
}

#[test]
fn paywall_denies_insufficient_amount() {
    let owner = owner_ph(synthetic_for(9));
    let nonce = payment_nonce(b"u");
    let obs = observed(owner, 100, PaymentAsset::Xch, Some(nonce));
    assert!(matches!(
        verify_payment_receipt(&obs, owner, 250, PaymentAsset::Xch, Some(nonce)),
        Err(PaywallError::InsufficientAmount { .. })
    ));
}

#[test]
fn paywall_denies_wrong_asset() {
    let owner = owner_ph(synthetic_for(9));
    let nonce = payment_nonce(b"u");
    let dig = PaymentAsset::Cat(Bytes32::new([0xABu8; 32]));
    let obs = observed(owner, 250, PaymentAsset::Xch, Some(nonce));
    assert!(matches!(
        verify_payment_receipt(&obs, owner, 250, dig, Some(nonce)),
        Err(PaywallError::WrongAsset { .. })
    ));
}

#[test]
fn paywall_denies_nonce_mismatch_and_replay() {
    let owner = owner_ph(synthetic_for(9));
    let issued = payment_nonce(b"unlock-A");
    let replayed = payment_nonce(b"unlock-B");
    let obs = observed(owner, 250, PaymentAsset::Xch, Some(replayed));
    assert!(matches!(
        verify_payment_receipt(&obs, owner, 250, PaymentAsset::Xch, Some(issued)),
        Err(PaywallError::NonceMismatch { .. })
    ));
}

#[test]
fn paywall_nonce_optional_when_not_required() {
    let owner = owner_ph(synthetic_for(9));
    let obs = observed(owner, 250, PaymentAsset::Xch, None);
    assert!(verify_payment_receipt(&obs, owner, 250, PaymentAsset::Xch, None).is_ok());
}

#[test]
fn paywall_end_to_end_receipt_self_verifies() {
    // Build a payment, then verify the receipt it produced (the dapp-side round trip): an observed
    // payment matching the receipt unlocks.
    let buyer = synthetic_for(2);
    let buyer_ph = owner_ph(buyer);
    let owner = owner_ph(synthetic_for(9));
    let nonce = payment_nonce(b"dapp:x:user:y");
    let r = build_xch_payment(buyer, vec![coin(buyer_ph, 1000)], owner, 250, nonce, 0).unwrap();

    let obs = ObservedPayment {
        paid_to_puzzle_hash: r.receipt.payment_coin.puzzle_hash,
        amount: r.receipt.payment_coin.amount,
        asset: r.receipt.asset,
        nonce: Some(r.receipt.nonce),
    };
    assert!(verify_payment_receipt(
        &obs,
        r.receipt.owner_puzzle_hash,
        250,
        r.receipt.asset,
        Some(r.receipt.nonce)
    )
    .is_ok());
}

// ---------------------------------------------------------------------------
// NFT-gating — prove ownership / collection membership
// ---------------------------------------------------------------------------

fn dig_media() -> NftMediaMetadata {
    NftMediaMetadata {
        data_uris: vec!["dig://urn:dig:chia:s:r/a.png".into()],
        data_hash: Some(Bytes32::new([1u8; 32])),
        metadata_uris: vec!["dig://urn:dig:chia:s:r/m.json".into()],
        metadata_hash: Some(Bytes32::new([2u8; 32])),
        license_uris: vec![],
        license_hash: None,
        edition_number: 1,
        edition_total: 1,
    }
}

/// Mint an NFT and return the eve coin spend that creates the current NFT coin (the parent spend the
/// gating helpers parse), plus the recipient ph and the attributed DID launcher id.
fn minted_nft_parent_spend(
    minter: PublicKey,
    recipient_ph: Bytes32,
    did: Option<DidAttribution>,
) -> chip35_dl_coin::CoinSpend {
    let params = NftMintParams {
        metadata: dig_media(),
        p2_puzzle_hash: recipient_ph,
        royalty_puzzle_hash: recipient_ph,
        royalty_basis_points: 0,
        did,
    };
    let r = mint_nft(minter, vec![coin(owner_ph(minter), 2)], params, 0).expect("mint_nft");
    // The NFT eve singleton spend is the one whose output coin is the minted NFT coin. Find the spend
    // whose coin is the NFT's parent (the singleton eve coin), i.e. the spend that produced
    // `r.nft_coin`. The NFT coin's parent_coin_info is the eve coin id.
    r.coin_spends
        .into_iter()
        .find(|cs| cs.coin.coin_id() == r.nft_coin.parent_coin_info)
        .expect("eve NFT spend present")
}

#[test]
fn prove_nft_ownership_reads_owner_and_launcher() {
    let minter = synthetic_for(2);
    let recipient = owner_ph(synthetic_for(4));
    let spend = minted_nft_parent_spend(minter, recipient, None);

    let proof = read_nft_ownership(&spend).expect("read");
    assert_eq!(proof.owner_puzzle_hash, recipient);
    assert_ne!(proof.launcher_id, Bytes32::default());

    // prove_nft_ownership succeeds for the correct owner.
    let ok = prove_nft_ownership(&spend, recipient, None).expect("owner matches");
    assert_eq!(ok.launcher_id, proof.launcher_id);

    // And can gate on a specific NFT launcher id.
    assert!(prove_nft_ownership(&spend, recipient, Some(proof.launcher_id)).is_ok());
}

#[test]
fn prove_nft_ownership_denies_wrong_owner() {
    let minter = synthetic_for(2);
    let recipient = owner_ph(synthetic_for(4));
    let stranger = owner_ph(synthetic_for(5));
    let spend = minted_nft_parent_spend(minter, recipient, None);
    assert!(matches!(
        prove_nft_ownership(&spend, stranger, None),
        Err(GatingError::WrongOwner { .. })
    ));
}

#[test]
fn prove_nft_ownership_denies_wrong_specific_nft() {
    let minter = synthetic_for(2);
    let recipient = owner_ph(synthetic_for(4));
    let spend = minted_nft_parent_spend(minter, recipient, None);
    assert!(matches!(
        prove_nft_ownership(&spend, recipient, Some(Bytes32::new([0xFFu8; 32]))),
        Err(GatingError::WrongNft { .. })
    ));
}

#[test]
fn prove_collection_membership_checks_attributed_did() {
    let minter = synthetic_for(2);
    let recipient = owner_ph(synthetic_for(4));

    // Make a real DID to attribute the mint to.
    let did_resp = create_did(minter, vec![coin(owner_ph(minter), 2)], 0).expect("create_did");
    let did = DidAttribution {
        launcher_id: did_resp.launcher_id,
        inner_puzzle_hash: did_resp.inner_puzzle_hash,
    };
    let spend = minted_nft_parent_spend(minter, recipient, Some(did.clone()));

    // Correct collection DID → membership proven.
    let proof = prove_collection_membership(&spend, recipient, did.launcher_id)
        .expect("member of the collection");
    assert_eq!(proof.attributed_did, Some(did.launcher_id));

    // Wrong collection DID → denied.
    assert!(matches!(
        prove_collection_membership(&spend, recipient, Bytes32::new([0xEEu8; 32])),
        Err(GatingError::WrongCollection { .. })
    ));
}

#[test]
fn gating_rejects_non_nft_spend() {
    // A plain XCH payment spend is not an NFT spend.
    let buyer = synthetic_for(2);
    let buyer_ph = owner_ph(buyer);
    let owner = owner_ph(synthetic_for(9));
    let r = build_xch_payment(
        buyer,
        vec![coin(buyer_ph, 100)],
        owner,
        10,
        payment_nonce(b"n"),
        0,
    )
    .unwrap();
    let not_nft = r.coin_spends.into_iter().next().unwrap();
    assert!(matches!(
        read_nft_ownership(&not_nft),
        Err(GatingError::NotAnNft)
    ));
}

// ---------------------------------------------------------------------------
// Subscription scaffold — must NOT silently fake a recurring spend.
// ---------------------------------------------------------------------------

#[test]
fn subscription_builders_are_scaffolded_not_implemented() {
    let buyer = synthetic_for(2);
    let terms = SubscriptionTerms {
        payee_puzzle_hash: owner_ph(synthetic_for(9)),
        amount_per_period: 100,
        asset_id: None,
        period_seconds: 2_592_000, // ~30 days
        max_periods: 12,
    };
    assert!(matches!(
        build_subscription_authorization(buyer, vec![coin(owner_ph(buyer), 2000)], terms, 0),
        Err(Error::Parse(_))
    ));
    assert!(matches!(
        build_subscription_claim(buyer, coin(owner_ph(buyer), 100), terms),
        Err(Error::Parse(_))
    ));
    // Keep SpendContext referenced so the import is used in the no-simulator style.
    let _ctx = SpendContext::new();
}
