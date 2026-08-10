//! Golden vectors — the exact bytes this crate builds, pinned.
//!
//! # Why this file exists
//! Every builder here produces a spend bundle that moves real money or anchors a real store. A
//! dependency migration (a chia / chia-sdk line bump), a refactor, or a driver-internals change can
//! silently alter *what gets signed* while every structural assertion in the rest of the suite still
//! passes — those tests check shape (`!coin_spends.is_empty()`, "determinism vs itself"), which is
//! invariant under exactly the kind of change that is dangerous here.
//!
//! These vectors are the byte-level baseline instead. Each one:
//!   * pins the **sha256 of the serialized [`SpendBundle`]** — maximally sensitive, so any change to
//!     a puzzle reveal, a solution, a condition, a memo, an ordering, or a coin amount trips it, and
//!   * pins the **per-coin-spend coin id + puzzle hash** and the builder's returned identifiers, so
//!     a tripped digest *localizes* instead of merely failing.
//!
//! # If one of these fails
//! **Do not update the expected value to make the test pass.** A changed vector means the code now
//! signs different bytes than the released crate did. That is a decision to be made and documented
//! (SPEC.md + the PR's `## Behaviour changes` section), never an adjustment.
//!
//! # Fixture design
//! Inputs are fully deterministic and chosen so the *nearest wrong implementation* is visible:
//!   * keys come from fixed seeds via the real `master_to_wallet_unhardened(..).derive_synthetic()`
//!     path, so a change to key derivation trips the vectors too;
//!   * every multi-coin vector uses **more than one** coin, because a single-coin fixture cannot see
//!     a change in ring ordering, in the lead-coin/follower condition split, or in concurrent-spend
//!     assertions;
//!   * the CAT payment leaves **change** (`total > amount`), because a no-change fixture cannot see
//!     the change output's hint or its position among the created coins;
//!   * the store mint carries a delegated puzzle, a label, a description and a size, because an
//!     all-`None` mint cannot see the metadata encoding it is supposed to pin.

use chia_puzzle_types::{standard::StandardArgs, DeriveSynthetic};
use chia_sdk_driver::{Cat, CatInfo, Launcher, SpendContext, StandardLayer};
use chip35_dl_coin::{
    build_dig_store_payment, build_lazy_mint_claim, build_lazy_mint_commit, build_xch_payment,
    create_did, master_to_wallet_unhardened, mint_store, sha256, spend_bundle_to_hex, Bytes32,
    Coin, CoinSpend, Collection, DelegatedPuzzle, LazyMintItem, LazyMintPolicy, NftMediaMetadata,
    PublicKey, SecretKey, Signature, SpendBundle, DIG_ASSET_ID,
};

// ---------------------------------------------------------------------------
// Deterministic fixture inputs
// ---------------------------------------------------------------------------

/// A synthetic wallet key from a fixed seed, derived the way a real wallet derives it.
fn synthetic(seed: u8) -> PublicKey {
    let sk = SecretKey::from_seed(&[seed; 32]);
    master_to_wallet_unhardened(&sk.public_key(), 0).derive_synthetic()
}

fn puzzle_hash(key: PublicKey) -> Bytes32 {
    StandardArgs::curry_tree_hash(key).into()
}

fn coin(parent: u8, ph: Bytes32, amount: u64) -> Coin {
    Coin {
        parent_coin_info: Bytes32::new([parent; 32]),
        puzzle_hash: ph,
        amount,
    }
}

fn dig_cat(parent: u8, owner_ph: Bytes32, amount: u64) -> Cat {
    Cat::new(
        coin(parent, owner_ph, amount),
        None,
        CatInfo::new(DIG_ASSET_ID, None, owner_ph),
    )
}

// ---------------------------------------------------------------------------
// The vector assertion
// ---------------------------------------------------------------------------

/// Assert a built spend matches its pinned digest, reporting the per-spend detail on failure so a
/// mismatch says *which* spend moved rather than only *that* something did.
///
/// The digest is taken over the canonical serialization (`spend_bundle_to_hex`) of an
/// aggregate-signature-free bundle: the builders are keyless, so the signature is the caller's and
/// is not part of what this crate produces.
fn assert_vector(name: &str, coin_spends: &[CoinSpend], expected_digest: &str) {
    let bundle = SpendBundle::new(coin_spends.to_vec(), Signature::default());
    let serialized = spend_bundle_to_hex(&bundle).expect("serialize spend bundle");
    let digest = hex::encode(sha256(serialized.as_bytes()));

    if digest != expected_digest {
        let detail: Vec<String> = coin_spends
            .iter()
            .enumerate()
            .map(|(i, cs)| {
                format!(
                    "  [{i}] coin_id={} puzzle_hash={} amount={}",
                    hex::encode(cs.coin.coin_id()),
                    hex::encode(cs.coin.puzzle_hash),
                    cs.coin.amount
                )
            })
            .collect();
        panic!(
            "GOLDEN VECTOR CHANGED: {name}\n  expected {expected_digest}\n  actual   {digest}\n\
             {} coin spends:\n{}\n\n\
             Do NOT update the expected value. Different bytes here means this crate now signs\n\
             something different than the released version did — record it under `## Behaviour\n\
             changes` and in SPEC.md, and get it decided.",
            coin_spends.len(),
            detail.join("\n")
        );
    }
}

// ---------------------------------------------------------------------------
// Vector: DataLayer store mint (the store coin spend)
// ---------------------------------------------------------------------------

/// Two funding coins (so ordering and the follower's concurrent-spend assertion are covered), full
/// metadata, an admin delegated puzzle, and a non-zero fee.
fn mint_store_vector() -> Vec<CoinSpend> {
    let minter = synthetic(2);
    let owner_ph = puzzle_hash(minter);
    mint_store(
        minter,
        vec![coin(7, owner_ph, 1_000), coin(8, owner_ph, 500)],
        Bytes32::new([3u8; 32]),
        Some("golden-label".into()),
        Some("golden-description".into()),
        Some(4_096),
        None,
        owner_ph,
        vec![DelegatedPuzzle::Admin(StandardArgs::curry_tree_hash(
            synthetic(9),
        ))],
        10,
    )
    .expect("mint_store")
    .coin_spends
}

#[test]
fn golden_mint_store() {
    assert_vector(
        "mint_store (DataLayer store coin launch)",
        &mint_store_vector(),
        MINT_STORE_DIGEST,
    );
}

// ---------------------------------------------------------------------------
// Vector: DIG CAT per-capsule store payment
// ---------------------------------------------------------------------------

/// Two DIG CATs totalling 900 paying 600 — so the ring has a follower AND the lead emits a change
/// output. A one-coin, exact-amount fixture would see neither.
fn dig_payment_vector() -> Vec<CoinSpend> {
    let buyer = synthetic(4);
    let buyer_ph = puzzle_hash(buyer);
    build_dig_store_payment(
        buyer,
        vec![dig_cat(11, buyer_ph, 600), dig_cat(12, buyer_ph, 300)],
        Bytes32::new([5u8; 32]),
        600,
    )
    .expect("build_dig_store_payment")
}

#[test]
fn golden_dig_store_payment() {
    assert_vector(
        "build_dig_store_payment (per-capsule $DIG to treasury)",
        &dig_payment_vector(),
        DIG_PAYMENT_DIGEST,
    );
}

// ---------------------------------------------------------------------------
// Vector: XCH payment (the plain-transfer path)
// ---------------------------------------------------------------------------

fn xch_payment_vector() -> Vec<CoinSpend> {
    let buyer = synthetic(6);
    let buyer_ph = puzzle_hash(buyer);
    build_xch_payment(
        buyer,
        vec![coin(13, buyer_ph, 1_000), coin(14, buyer_ph, 250)],
        puzzle_hash(synthetic(7)),
        900,
        Bytes32::new([15u8; 32]),
        5,
    )
    .expect("build_xch_payment")
    .coin_spends
}

#[test]
fn golden_xch_payment() {
    assert_vector(
        "build_xch_payment (XCH transfer with change + fee)",
        &xch_payment_vector(),
        XCH_PAYMENT_DIGEST,
    );
}

// ---------------------------------------------------------------------------
// Vector: DID creation
// ---------------------------------------------------------------------------

fn create_did_vector() -> Vec<CoinSpend> {
    let minter = synthetic(16);
    let minter_ph = puzzle_hash(minter);
    create_did(
        minter,
        vec![coin(17, minter_ph, 1_000), coin(18, minter_ph, 100)],
        20,
    )
    .expect("create_did")
    .coin_spends
}

#[test]
fn golden_create_did() {
    assert_vector(
        "create_did (DID singleton launch)",
        &create_did_vector(),
        CREATE_DID_DIGEST,
    );
}

// ---------------------------------------------------------------------------
// Vectors: lazy mint commit + claim
// ---------------------------------------------------------------------------

fn lazy_item(i: usize) -> LazyMintItem {
    LazyMintItem {
        metadata: NftMediaMetadata {
            data_uris: vec![format!("urn:dig:chia:golden:root/item{i}.png")],
            data_hash: Some(sha256(format!("golden-bytes-{i}").as_bytes())),
            metadata_uris: vec![format!("urn:dig:chia:golden:root/item{i}.json")],
            metadata_hash: Some(sha256(format!("golden-meta-{i}").as_bytes())),
            license_uris: vec![],
            license_hash: None,
            edition_number: 1,
            edition_total: 2,
        },
        royalty_basis_points: 300,
    }
}

fn lazy_collection(royalty_ph: Bytes32) -> Collection {
    Collection {
        id: "golden-col".into(),
        name: "Golden Collection".into(),
        attributes: vec![],
        royalty_puzzle_hash: royalty_ph,
        royalty_basis_points: 300,
    }
}

/// A DID built from a fixed launcher parent, used as the commit authority.
fn golden_did(creator: PublicKey) -> chip35_dl_coin::Did {
    let ctx = &mut SpendContext::new();
    let p2 = StandardLayer::new(creator);
    let (_conditions, did) = Launcher::new(coin(19, puzzle_hash(creator), 1).coin_id(), 1)
        .create_simple_did(ctx, &p2)
        .expect("create_simple_did");
    did
}

/// Two items, so the commitment coins' ordering and per-index puzzle hashes are covered.
fn lazy_commit() -> chip35_dl_coin::LazyMintCommitResponse {
    let creator = synthetic(20);
    build_lazy_mint_commit(
        creator,
        golden_did(creator),
        &lazy_collection(puzzle_hash(creator)),
        &[lazy_item(0), lazy_item(1)],
        LazyMintPolicy::DirectMint,
        None,
    )
    .expect("build_lazy_mint_commit")
}

#[test]
fn golden_lazy_mint_commit() {
    assert_vector(
        "build_lazy_mint_commit (creator precommit)",
        &lazy_commit().coin_spends,
        LAZY_MINT_COMMIT_DIGEST,
    );
}

#[test]
fn golden_lazy_mint_claim() {
    let commit = lazy_commit();
    let claimer = synthetic(21);
    let claimer_ph = puzzle_hash(claimer);
    let claim = build_lazy_mint_claim(
        claimer,
        vec![coin(22, claimer_ph, 1_000), coin(23, claimer_ph, 50)],
        claimer_ph,
        &commit.descriptor(),
        1,
        None,
        7,
    )
    .expect("build_lazy_mint_claim");

    assert_vector(
        "build_lazy_mint_claim (mint-on-claim, item index 1)",
        &claim.coin_spends,
        LAZY_MINT_CLAIM_DIGEST,
    );
}

// ---------------------------------------------------------------------------
// Pinned digests — sha256 of the canonical serialization of each bundle above.
//
// Established on the chia 0.26 / chia-sdk 0.30 dependency line (crate 0.9.0), BEFORE any migration.
// A migration is correct exactly when these still hold.
// ---------------------------------------------------------------------------

const MINT_STORE_DIGEST: &str = "a6148f4916bd5a98b0eeb5a94dac93eea71eb067fa4c1bb34ed57e2781c7c017";
const DIG_PAYMENT_DIGEST: &str = "24a7878357a682bc871e7837a3decf48577a33a913ea7a2edddd02abceb5928a";
const XCH_PAYMENT_DIGEST: &str = "e3ab0ce2faa31b1bad741160f6daf656fb96fe2dc7b2b32a218944f3c4c361fb";
const CREATE_DID_DIGEST: &str = "459a29ee8cfaf62789b20b5f3a23c573f6053135f2bd000c312db403a5a8dc3c";
const LAZY_MINT_COMMIT_DIGEST: &str =
    "979faf927b31d9d13169a455731978fd95fbf57b5529c49f092e3c5576d2aee0";
const LAZY_MINT_CLAIM_DIGEST: &str =
    "6a7e95c4cec6ba28b3b18448633d511cdf14d0fd2eb04a38f18d708a2cc34514";
