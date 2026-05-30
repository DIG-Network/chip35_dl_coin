//! Prints deterministic test inputs (a synthetic public key and its standard
//! puzzle hash) as JSON, for the Node parity test. Run:
//!   cargo run -p chip35-dl-coin --example dump_fixtures > wasm/tests/fixtures.json

use chia_bls::{master_to_wallet_unhardened, SecretKey};
use chia_protocol::Bytes32;
use chia_puzzle_types::{standard::StandardArgs, DeriveSynthetic};

fn main() {
    let sk = SecretKey::from_seed(&[2u8; 32]);
    let synthetic = master_to_wallet_unhardened(&sk.public_key(), 0).derive_synthetic();
    let ph: Bytes32 = StandardArgs::curry_tree_hash(synthetic).into();

    println!("{{");
    println!("  \"syntheticKeyHex\": \"{}\",", hex::encode(synthetic.to_bytes()));
    println!("  \"puzzleHashHex\": \"{}\",", hex::encode(ph));
    println!("  \"parentCoinInfoHex\": \"{}\",", hex::encode([7u8; 32]));
    println!("  \"rootHashHex\": \"{}\"", hex::encode([3u8; 32]));
    println!("}}");
}
