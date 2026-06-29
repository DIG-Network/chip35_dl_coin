//! Tests for the asset toolkit spend builders: NFT mint (#33), collection bulk mint (#34),
//! DID/CAT/offer (#35). These assert the builders produce well-formed, deterministic coin spends
//! at the keyless boundary (the existing `builders.rs` style — no simulator).

use chia_puzzle_types::{standard::StandardArgs, DeriveSynthetic};
use chia_sdk_driver::{Launcher, SpendContext, StandardLayer};
use chip35_dl_coin::{
    build_bulk_mint, create_did, decode_offer, encode_offer, generate_item_metadata, issue_cat,
    master_to_wallet_unhardened, mint_nft, mint_nft_with_did, sha256, spend_bundle_to_hex,
    Attribute, Bytes32, Coin, Collection, DidAttribution, Error, ManifestItem, ManifestMedia,
    NftMediaMetadata, NftMintParams, PublicKey, SecretKey, Signature, SpendBundle,
};

fn synthetic() -> PublicKey {
    let sk = SecretKey::from_seed(&[2u8; 32]);
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

fn dig_media() -> NftMediaMetadata {
    let data = b"the real PNG bytes stored in a DIG capsule";
    let meta = b"{\"format\":\"CHIP-0007\",\"name\":\"x\"}";
    NftMediaMetadata {
        data_uris: vec![
            "dig://urn:dig:chia:store123:root456/art.png".into(),
            "https://gateway.dig.net/store123/root456/art.png".into(),
        ],
        data_hash: Some(sha256(data)),
        metadata_uris: vec![
            "dig://urn:dig:chia:store123:root456/metadata.json".into(),
            "https://gateway.dig.net/store123/root456/metadata.json".into(),
        ],
        metadata_hash: Some(sha256(meta)),
        license_uris: vec![],
        license_hash: None,
        edition_number: 1,
        edition_total: 1,
    }
}

// ---- #33: single NFT mint ----

#[test]
fn mint_nft_produces_spends_and_is_deterministic() {
    let synth = synthetic();
    let ph = owner_ph(synth);
    let params = NftMintParams {
        metadata: dig_media(),
        p2_puzzle_hash: ph,
        royalty_puzzle_hash: ph,
        royalty_basis_points: 300,
        did: None,
    };

    let r1 = mint_nft(synth, vec![coin(ph, 2)], params.clone(), 0).expect("mint_nft");
    assert!(!r1.coin_spends.is_empty());
    assert_ne!(r1.launcher_id, Bytes32::default());

    let r2 = mint_nft(synth, vec![coin(ph, 2)], params, 0).expect("mint_nft 2");
    let hex1 =
        spend_bundle_to_hex(&SpendBundle::new(r1.coin_spends, Signature::default())).unwrap();
    let hex2 =
        spend_bundle_to_hex(&SpendBundle::new(r2.coin_spends, Signature::default())).unwrap();
    assert_eq!(hex1, hex2, "mint is deterministic for identical inputs");
}

#[test]
fn mint_nft_with_did_attribution_succeeds() {
    let synth = synthetic();
    let ph = owner_ph(synth);
    let params = NftMintParams {
        metadata: dig_media(),
        p2_puzzle_hash: ph,
        royalty_puzzle_hash: ph,
        royalty_basis_points: 250,
        did: Some(DidAttribution {
            launcher_id: Bytes32::new([1u8; 32]),
            inner_puzzle_hash: Bytes32::new([2u8; 32]),
        }),
    };
    let r = mint_nft(synth, vec![coin(ph, 2)], params, 0).expect("mint with DID");
    assert!(!r.coin_spends.is_empty());
}

// ---- #38: single NFT mint AUTHORIZED BY + attributed to a creator DID ----
// mint_nft only adds the TransferNft attribution; the DID must be spent elsewhere to authorize it.
// mint_nft_with_did composes that DID-acknowledgement spend INTO the mint bundle so the on-chain
// owner assignment is actually authorized by the creator identity (roadmap #38).

#[test]
fn mint_nft_with_did_spends_the_did_coin() {
    let synth = synthetic();
    let ph = owner_ph(synth);

    // A real DID the creator mints under.
    let ctx = &mut SpendContext::new();
    let p2 = StandardLayer::new(synth);
    let did_lead = coin(ph, 1);
    let (_c, did) = Launcher::new(did_lead.coin_id(), 1)
        .create_simple_did(ctx, &p2)
        .expect("create did");
    let did_coin_id = did.coin.coin_id();

    let params = NftMintParams {
        metadata: dig_media(),
        p2_puzzle_hash: ph,
        royalty_puzzle_hash: ph,
        royalty_basis_points: 250,
        did: None, // the dedicated builder takes the full Did; this field is ignored here
    };

    let r = mint_nft_with_did(synth, vec![coin(ph, 2)], did, params, 0).expect("mint as did");
    assert!(!r.coin_spends.is_empty());
    assert_ne!(r.launcher_id, Bytes32::default());
    // The creator DID coin MUST be spent in the bundle (authorizing the attribution), not just named.
    assert!(
        r.coin_spends
            .iter()
            .any(|cs| cs.coin.coin_id() == did_coin_id),
        "the creator DID coin is spent to authorize the mint"
    );
}

#[test]
fn mint_nft_with_did_is_deterministic() {
    let synth = synthetic();
    let ph = owner_ph(synth);
    let did_lead = coin(ph, 1);

    let build = || {
        let ctx = &mut SpendContext::new();
        let p2 = StandardLayer::new(synth);
        let (_c, did) = Launcher::new(did_lead.coin_id(), 1)
            .create_simple_did(ctx, &p2)
            .unwrap();
        let params = NftMintParams {
            metadata: dig_media(),
            p2_puzzle_hash: ph,
            royalty_puzzle_hash: ph,
            royalty_basis_points: 250,
            did: None,
        };
        let r = mint_nft_with_did(synth, vec![coin(ph, 2)], did, params, 0).unwrap();
        spend_bundle_to_hex(&SpendBundle::new(r.coin_spends, Signature::default())).unwrap()
    };
    assert_eq!(build(), build(), "DID mint is deterministic for identical inputs");
}

#[test]
fn mint_nft_with_did_no_coins_errors() {
    let synth = synthetic();
    let ph = owner_ph(synth);
    let ctx = &mut SpendContext::new();
    let p2 = StandardLayer::new(synth);
    let (_c, did) = Launcher::new(coin(ph, 1).coin_id(), 1)
        .create_simple_did(ctx, &p2)
        .unwrap();
    let params = NftMintParams {
        metadata: dig_media(),
        p2_puzzle_hash: ph,
        royalty_puzzle_hash: ph,
        royalty_basis_points: 0,
        did: None,
    };
    assert!(matches!(
        mint_nft_with_did(synth, vec![], did, params, 0),
        Err(Error::Parse(_))
    ));
}

#[test]
fn mint_nft_with_no_coins_errors() {
    let synth = synthetic();
    let ph = owner_ph(synth);
    let params = NftMintParams {
        metadata: dig_media(),
        p2_puzzle_hash: ph,
        royalty_puzzle_hash: ph,
        royalty_basis_points: 0,
        did: None,
    };
    assert!(matches!(
        mint_nft(synth, vec![], params, 0),
        Err(Error::Parse(_))
    ));
}

// ---- #35: DID ----

#[test]
fn create_did_produces_spends() {
    let synth = synthetic();
    let ph = owner_ph(synth);
    let r = create_did(synth, vec![coin(ph, 2)], 0).expect("create_did");
    assert!(!r.coin_spends.is_empty());
    assert_ne!(r.launcher_id, Bytes32::default());
    assert_ne!(r.inner_puzzle_hash, Bytes32::default());
}

#[test]
fn create_did_with_no_coins_errors() {
    let synth = synthetic();
    assert!(matches!(create_did(synth, vec![], 0), Err(Error::Parse(_))));
}

// ---- #35: CAT ----

#[test]
fn issue_cat_produces_spends_and_asset_id() {
    let synth = synthetic();
    let ph = owner_ph(synth);
    let r = issue_cat(synth, vec![coin(ph, 1000)], 1000, 0).expect("issue_cat");
    assert!(!r.coin_spends.is_empty());
    assert_ne!(r.asset_id, Bytes32::default());
    assert!(!r.cat_coins.is_empty());
}

#[test]
fn issue_cat_insufficient_funds_errors() {
    let synth = synthetic();
    let ph = owner_ph(synth);
    // amount 1000 + fee 10 > coin of 100
    assert!(matches!(
        issue_cat(synth, vec![coin(ph, 100)], 1000, 10),
        Err(Error::Parse(_))
    ));
}

// ---- #35: offer codec ----

#[test]
fn offer_encode_decode_roundtrip() {
    // A trivial spend bundle round-trips through the offer codec.
    let synth = synthetic();
    let ph = owner_ph(synth);
    let minted = mint_nft(
        synth,
        vec![coin(ph, 2)],
        NftMintParams {
            metadata: dig_media(),
            p2_puzzle_hash: ph,
            royalty_puzzle_hash: ph,
            royalty_basis_points: 0,
            did: None,
        },
        0,
    )
    .unwrap();
    let bundle = SpendBundle::new(minted.coin_spends, Signature::default());

    let text = encode_offer(&bundle).expect("encode");
    assert!(text.starts_with("offer1"), "offer text starts with offer1");
    let decoded = decode_offer(&text).expect("decode");
    assert_eq!(decoded.coin_spends.len(), bundle.coin_spends.len());
}

// ---- #34: collection metadata + bulk mint ----

fn manifest_items(n: usize) -> Vec<ManifestItem> {
    (0..n)
        .map(|i| ManifestItem {
            name: format!("DIG Punk #{}", i + 1),
            description: Some("a generated item".into()),
            attributes: vec![Attribute {
                trait_type: "Index".into(),
                value: i.to_string(),
            }],
            media: ManifestMedia {
                data_uris: vec![format!("dig://urn:dig:chia:store:root/item{i}.png")],
                data_hash: Some(sha256(format!("bytes-{i}").as_bytes())),
                metadata_uris: vec![format!("dig://urn:dig:chia:store:root/item{i}.json")],
                metadata_hash: Some(sha256(format!("meta-{i}").as_bytes())),
                license_uris: vec![],
                license_hash: None,
            },
        })
        .collect()
}

fn test_collection(synth: PublicKey) -> Collection {
    Collection {
        id: "col-abc".into(),
        name: "DIG Punks".into(),
        attributes: vec![Attribute {
            trait_type: "website".into(),
            value: "https://dig.net".into(),
        }],
        royalty_puzzle_hash: owner_ph(synth),
        royalty_basis_points: 420,
    }
}

#[test]
fn generate_item_metadata_fills_series_and_collection() {
    let synth = synthetic();
    let col = test_collection(synth);
    let items = manifest_items(3);
    let docs = generate_item_metadata(&col, &items);
    assert_eq!(docs.len(), 3);
    for (i, doc) in docs.iter().enumerate() {
        doc.validate_schema()
            .expect("generated doc is valid CHIP-0007");
        assert_eq!(doc.series_number, Some(i as u64 + 1));
        assert_eq!(doc.series_total, Some(3));
        let cref = doc.collection.as_ref().expect("collection block present");
        assert_eq!(cref.id, "col-abc");
        assert_eq!(cref.name, "DIG Punks");
    }
}

#[test]
fn build_bulk_mint_produces_spends_for_all_items() {
    let synth = synthetic();
    let col = test_collection(synth);
    let items = manifest_items(2);

    // Build a real DID to attribute the mints to.
    let ctx = &mut SpendContext::new();
    let p2 = StandardLayer::new(synth);
    let did_lead = coin(owner_ph(synth), 1);
    let (_create_did, did) = Launcher::new(did_lead.coin_id(), 1)
        .create_simple_did(ctx, &p2)
        .expect("create did for test");

    let r = build_bulk_mint(synth, did, &col, &items, owner_ph(synth)).expect("bulk mint");
    assert!(!r.coin_spends.is_empty());
    assert_eq!(r.launcher_ids.len(), 2, "one launcher id per item");
    assert_ne!(r.launcher_ids[0], r.launcher_ids[1], "items are distinct");
}

#[test]
fn build_bulk_mint_empty_manifest_errors() {
    let synth = synthetic();
    let col = test_collection(synth);
    let ctx = &mut SpendContext::new();
    let p2 = StandardLayer::new(synth);
    let did_lead = coin(owner_ph(synth), 1);
    let (_c, did) = Launcher::new(did_lead.coin_id(), 1)
        .create_simple_did(ctx, &p2)
        .unwrap();
    assert!(matches!(
        build_bulk_mint(synth, did, &col, &[], owner_ph(synth)),
        Err(Error::Parse(_))
    ));
}
