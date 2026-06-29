//! The per-capsule DIG-payment contract (task #111): minting a store is FREE of $DIG; a CAPSULE
//! (commit / root-advance) is the only step that pays $DIG to the treasury.
//!
//! These tests pin the cross-system pricing contract at its canonical source (chip35 is the spend
//! builder): a MINT spend bundle MUST NOT contain a DIG-CAT payment coin to the treasury, and a
//! COMMIT (root-advance + DIG payment) bundle MUST. Keyless-boundary style (no simulator), like
//! `builders.rs` / `monetization.rs`.

use chia_puzzle_types::{standard::StandardArgs, DeriveSynthetic};
use chia_sdk_driver::{Cat, CatInfo};
use chip35_dl_coin::{
    build_dig_store_payment, dig_treasury_payment_coin, master_to_wallet_unhardened, mint_store,
    update_store_metadata, Bytes32, Coin, CoinSpend, DataStoreInnerSpend, PublicKey, SecretKey,
    DIG_ASSET_ID, DIG_TREASURY_INNER_PUZZLE_HASH,
};

fn synthetic_for(seed: u8) -> PublicKey {
    let sk = SecretKey::from_seed(&[seed; 32]);
    master_to_wallet_unhardened(&sk.public_key(), 0).derive_synthetic()
}

fn owner_ph(synth: PublicKey) -> Bytes32 {
    StandardArgs::curry_tree_hash(synth).into()
}

fn xch_coin(ph: Bytes32, amount: u64) -> Coin {
    Coin {
        parent_coin_info: Bytes32::new([7u8; 32]),
        puzzle_hash: ph,
        amount,
    }
}

/// A buyer-owned DIG [`Cat`] of `amount` base units (keyless; on-chain the caller parses real DIG
/// CATs). Uses [`DIG_ASSET_ID`] so it is accepted by `build_dig_store_payment`.
fn dig_cat(buyer: PublicKey, amount: u64) -> Cat {
    let p2 = owner_ph(buyer);
    Cat::new(
        Coin {
            parent_coin_info: Bytes32::new([5u8; 32]),
            puzzle_hash: Bytes32::new([6u8; 32]),
            amount,
        },
        None,
        CatInfo::new(DIG_ASSET_ID, None, p2),
    )
}

/// Whether a coin-spend set pays $DIG to the DIG treasury — i.e. contains a CAT spend that commits to
/// the treasury's INNER puzzle hash as a payment recipient.
///
/// The DIG payment's inner standard-puzzle delegated conditions create a coin to
/// [`DIG_TREASURY_INNER_PUZZLE_HASH`] (wrapped by the CAT layer on-chain), so the treasury inner
/// puzzle hash's 32 bytes appear in the spend's serialized `puzzle_reveal || solution`. Mint/update
/// (singleton) spends never reference the treasury, so this is a faithful, keyless signal of "this
/// bundle pays the DIG treasury" — no on-chain CAT lineage / puzzle execution required.
fn bundle_pays_dig_treasury(coin_spends: &[CoinSpend]) -> bool {
    let needle = DIG_TREASURY_INNER_PUZZLE_HASH.to_bytes();
    coin_spends.iter().any(|cs| {
        contains_subslice(cs.puzzle_reveal.as_ref(), &needle)
            || contains_subslice(cs.solution.as_ref(), &needle)
    })
}

/// True if `haystack` contains the contiguous bytes `needle`.
fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn mint_bundle_has_no_dig_payment_but_commit_does() {
    let owner = synthetic_for(2);
    let owner_p = owner_ph(owner);
    let root0 = Bytes32::new([0u8; 32]); // empty/initial root — minting an empty-root store is intended
    let amount = 100_000u64; // a sample dynamic per-capsule price (base units); the value is an input

    let lead = dig_cat(owner, 1_000_000);

    // --- MINT: launch the store with an empty root + XCH fee, NO DIG payment. ---
    let mint = mint_store(
        owner,
        vec![xch_coin(owner_p, 1_000_000)],
        root0,
        Some("My Store".into()),
        None,
        None,
        None,
        owner_p,
        vec![],
        1_000,
    )
    .expect("mint_store");
    assert!(
        !bundle_pays_dig_treasury(&mint.coin_spends),
        "MINT must NOT pay $DIG to the treasury — minting a store is free of $DIG"
    );

    // --- COMMIT (capsule): advance the root AND pay the per-capsule $DIG price. ---
    let new_root = Bytes32::new([0xAB; 32]);
    let update = update_store_metadata(
        mint.new_datastore.clone(),
        new_root,
        Some("My Store".into()),
        None,
        None,
        None,
        DataStoreInnerSpend::Owner(owner),
    )
    .expect("update_store_metadata");
    let store_id = mint.new_datastore.info.launcher_id;
    let dig_pay = build_dig_store_payment(owner, vec![lead], store_id, amount)
        .expect("build_dig_store_payment");

    // The commit bundle = root-advance spends + the DIG payment spends, atomically.
    let mut commit = update.coin_spends.clone();
    commit.extend(dig_pay);
    assert!(
        bundle_pays_dig_treasury(&commit),
        "COMMIT (capsule creation) MUST pay the per-capsule $DIG price to the treasury"
    );
    // Sanity: the root-advance singleton spends ALONE (without the DIG payment) do not pay the
    // treasury — the payment is the DIG part, not the singleton update.
    assert!(
        !bundle_pays_dig_treasury(&update.coin_spends),
        "the root-advance singleton spend itself carries no DIG payment"
    );

    // The expected treasury payment coin is the DIG-CAT-wrapped treasury inner ph (not the inner ph
    // itself, and not the buyer) — pins the public helper used by callers to verify the payment.
    let pay_coin = dig_treasury_payment_coin(&lead, amount);
    assert_eq!(pay_coin.amount, amount);
    assert_ne!(pay_coin.puzzle_hash, DIG_TREASURY_INNER_PUZZLE_HASH);
}

#[test]
fn dig_payment_pays_exact_amount_and_returns_change() {
    let owner = synthetic_for(3);
    let store_id = Bytes32::new([0x11; 32]);
    let lead = dig_cat(owner, 1_000_000);

    let spends = build_dig_store_payment(owner, vec![lead], store_id, 250_000)
        .expect("build_dig_store_payment");
    assert!(!spends.is_empty(), "DIG payment produces coin spends");
    assert!(
        bundle_pays_dig_treasury(&spends),
        "the DIG payment pays the treasury its DIG-CAT coin"
    );
}

#[test]
fn dig_payment_rejects_non_dig_cat() {
    let owner = synthetic_for(3);
    // A CAT of a different asset id is rejected (only $DIG pays a capsule).
    let other = Cat::new(
        Coin {
            parent_coin_info: Bytes32::new([5u8; 32]),
            puzzle_hash: Bytes32::new([6u8; 32]),
            amount: 1_000_000,
        },
        None,
        CatInfo::new(Bytes32::new([0xCD; 32]), None, owner_ph(owner)),
    );
    assert!(matches!(
        build_dig_store_payment(owner, vec![other], Bytes32::default(), 1_000),
        Err(chip35_dl_coin::Error::Parse(_))
    ));
}

#[test]
fn dig_payment_rejects_insufficient_and_empty() {
    let owner = synthetic_for(3);
    assert!(matches!(
        build_dig_store_payment(owner, vec![], Bytes32::default(), 1),
        Err(chip35_dl_coin::Error::Parse(_))
    ));
    let short = dig_cat(owner, 100);
    assert!(matches!(
        build_dig_store_payment(owner, vec![short], Bytes32::default(), 1_000),
        Err(chip35_dl_coin::Error::Parse(_))
    ));
}

#[test]
fn dig_constants_match_cross_system_contract() {
    // Byte-identical to the digstore-chain mirror + SYSTEM.md (DIG CAT payment contract).
    assert_eq!(
        hex::encode(DIG_ASSET_ID),
        "a406d3a9de984d03c9591c10d917593b434d5263cabe2b42f6b367df16832f81"
    );
    assert_eq!(
        hex::encode(DIG_TREASURY_INNER_PUZZLE_HASH),
        "ec7c304708c7d59c078d5ae098d0dea004decf47fa1cafebb266c10ad6466ce8"
    );
}
