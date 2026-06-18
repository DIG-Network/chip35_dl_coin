//! Prints deterministic test inputs (a synthetic public key and its standard
//! puzzle hash) as JSON, for the Node parity test. Run:
//!   cargo run -p chip35-dl-coin --example dump_fixtures > wasm/tests/fixtures.json

use chia_bls::{master_to_wallet_unhardened, SecretKey};
use chia_protocol::{Bytes32, SpendBundle};
use chia_puzzle_types::{standard::StandardArgs, DeriveSynthetic};
use chip35_dl_coin::{mint_store, spend_bundle_to_hex, Coin, DelegatedPuzzle, Signature};

fn main() {
    let sk = SecretKey::from_seed(&[2u8; 32]);
    let synthetic = master_to_wallet_unhardened(&sk.public_key(), 0).derive_synthetic();
    let ph: Bytes32 = StandardArgs::curry_tree_hash(synthetic).into();

    // Build the same mint the node/wasm test builds (byte-for-byte parity).
    // Inputs match builders.mjs exactly:
    //   lead coin: parent=[7;32], puzzle_hash=ph, amount=2
    //   rootHash=[3;32], label="label", description="desc", bytes=42
    //   sizeProof=None, ownerPuzzleHash=ph
    //   delegatedPuzzles=[Admin(curry_tree_hash(synthetic))], fee=0
    let coin = Coin {
        parent_coin_info: Bytes32::new([7u8; 32]),
        puzzle_hash: ph,
        amount: 2,
    };
    let admin = DelegatedPuzzle::Admin(StandardArgs::curry_tree_hash(synthetic));
    let mint = mint_store(
        synthetic,
        vec![coin],
        Bytes32::new([3u8; 32]),
        Some("label".into()),
        Some("desc".into()),
        Some(42),
        None,
        ph,
        vec![admin],
        0,
    )
    .expect("mint");

    // Identity (infinity) signature — same as the node test's `identitySig`.
    let mint_hex = spend_bundle_to_hex(&SpendBundle::new(
        mint.coin_spends.clone(),
        Signature::default(),
    ))
    .expect("hex");

    println!("{{");
    println!(
        "  \"syntheticKeyHex\": \"{}\",",
        hex::encode(synthetic.to_bytes())
    );
    println!("  \"puzzleHashHex\": \"{}\",", hex::encode(ph));
    println!("  \"parentCoinInfoHex\": \"{}\",", hex::encode([7u8; 32]));
    println!("  \"rootHashHex\": \"{}\",", hex::encode([3u8; 32]));
    println!("  \"mintHex\": \"{}\"", mint_hex);
    println!("}}");
}
