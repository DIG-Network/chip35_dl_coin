# chip35_dl_coin — Design

**Date:** 2026-05-30
**Status:** Approved (design), pending implementation plan
**Author:** Michael Taylor (with Claude Code)

## 1. Goal

Extract the **CHIP-0035 DataLayer store coin** puzzle/driver functionality from
`DataLayer-Driver` into a new, fully isolated project (`chip35_dl_coin`) and
expose it as a **WebAssembly module** that builds the coin spends required to
**mint, update, and burn** DataLayer stores (plus oracle spend).

**Primary success criterion:** in a browser/bundler (and Node, for tests)
environment, call `mintStore` / `updateStoreMetadata` / `updateStoreOwnership` /
`meltStore` / `oracleSpend` and receive the resulting `CoinSpend[]` (and updated
`DataStore` state) — byte-for-byte identical to what the original
`DataLayer-Driver` produces, since both rest on the same `chia-sdk-driver`
chip-0035 puzzle logic.

### Scope decisions (locked with the user)

- **Operations surface: spend builders only.** The module emits coin spends.
  The consumer handles BLS signing, coin selection, and key derivation with its
  own tooling.
- **Isolation: standalone.** The new crates depend *only* on upstream `chia-*`
  crates. There is **no dependency** on `DataLayer-Driver`, and none of its DIG
  extras (server coins, collateral, DID, NFT, `send_xch`) or networking is
  carried over.
- **WASM targets: bundler + Node.** Publish the `--target bundler` build; build
  `--target nodejs` for tests.
- **Layout: Cargo workspace** with a native-buildable `core` library crate and a
  thin `wasm` binding crate.

### The spend-bundle nuance (resolved)

A Chia *spend bundle* is `coinSpends + aggregatedSignature`. Because signing is
the consumer's responsibility, the module's builders return **coin spends**, not
a signed bundle. To let the consumer assemble/serialize the final bundle without
re-implementing CLVM serialization, the module **includes two keyless helpers**:

- `spendBundleToHex(coinSpends, aggregatedSignature) -> hex string`
- `hexSpendBundleToCoinSpends(hex) -> CoinSpend[]`

These touch no keys, no coin selection, no signing — the consumer signs
externally and passes the signature in.

## 2. Non-goals

- No networking (`Peer`, TLS, `connect_*`, `sync_*`, `broadcast_*`, any `async`
  `&Peer` fn). WASM has no native sockets; chain reads/writes stay the
  consumer's responsibility.
- No BLS signing inside the module (`signCoinSpends`, `signMessage`,
  `verifySignedMessage`) — consumer-side.
- No coin selection (`selectCoins`) — consumer-side.
- No key/address derivation (`master_*`, synthetic-key, puzzle-hash, address
  conversion) — consumer-side.
- No `adminDelegatedPuzzleFromKey` / `writerDelegatedPuzzleFromKey` (key-derivation
  adjacent — consumer supplies the inner puzzle hash when constructing a
  `DelegatedPuzzle`).
- No `getCost`, `addFee`, `sendXch`, server coins, collateral, DID, NFT.
- No `native` cargo feature — because zero networking code is copied, the core
  crate is wasm-safe by construction (a deliberate simplification vs. the
  original, which needed feature gating to hide its `Peer` code).

## 3. What gets extracted (core driver)

These functions are copied (trimmed) out of `DataLayer-Driver/src/wallet.rs` and
`src/lib.rs`. All are synchronous and offline. Source line references are for
extraction guidance.

| Function | Source | Signature (native) |
|---|---|---|
| `mint_store` | `wallet.rs:399` | `(minter_synthetic_key: PublicKey, selected_coins: Vec<Coin>, root_hash: Bytes32, label: Option<String>, description: Option<String>, bytes: Option<u64>, size_proof: Option<String>, owner_puzzle_hash: Bytes32, delegated_puzzles: Vec<DelegatedPuzzle>, fee: u64) -> Result<SuccessResponse, Error>` |
| `update_store_metadata` | `wallet.rs:794` | `(datastore: DataStore, new_root_hash: Bytes32, new_label: Option<String>, new_description: Option<String>, new_bytes: Option<u64>, new_size_proof: Option<String>, inner_spend_info: DataStoreInnerSpend) -> Result<SuccessResponse, Error>` |
| `update_store_ownership` | `wallet.rs:745` | `(datastore: DataStore, new_owner_puzzle_hash: Bytes32, new_delegated_puzzles: Vec<DelegatedPuzzle>, inner_spend_info: DataStoreInnerSpend) -> Result<SuccessResponse, Error>` |
| `melt_store` (burn) | `wallet.rs:838` | `(datastore: DataStore, owner_pk: PublicKey) -> Result<Vec<CoinSpend>, Error>` |
| `oracle_spend` | `wallet.rs:860` | `(spender_synthetic_key: PublicKey, selected_coins: Vec<Coin>, datastore: DataStore, fee: u64) -> Result<SuccessResponse, Error>` |
| `spend_bundle_to_hex` | `lib.rs:165` | `(spend_bundle: &SpendBundle) -> Result<String, Error>` |
| `hex_spend_bundle_to_coin_spends` | `lib.rs:157` | `(hex: &str) -> Result<Vec<CoinSpend>, Error>` |

**Private helpers to copy** (called by the above): the internal
`update_store_with_conditions` helper used by both `update_store_*`, and
`reserve_fee` used by `oracle_spend`. Any other private helper the compiler
flags as missing during extraction is copied in the same pass; helpers tied only
to excluded functions are dropped.

**Upstream calls relied upon** (all from `chia-sdk-driver` chip-0035 +
`chia-sdk-types`/`chia-sdk-utils`): `SpendContext`, `StandardLayer`,
`WriterLayer`, `OracleLayer`, `Launcher`/`Launcher::mint_datastore`,
`DataStore` (`spend`, `from_spend`, `owner_create_coin_condition`,
`new_metadata_condition`, `get_recreation_memos`), `get_merkle_tree`,
`Conditions`/`Condition`, `CreateCoin`, `MeltSingleton`,
`UpdateDataStoreMerkleRoot`, `announcement_id`, `StandardArgs::curry_tree_hash`,
`SINGLETON_LAUNCHER_HASH`.

### Local types to define (copied/trimmed)

```rust
// core/src/types.rs
pub struct SuccessResponse {
    pub coin_spends: Vec<CoinSpend>,
    pub new_datastore: DataStore,   // NOTE: field name is `new_datastore`
}

// core/src/lib.rs (was wallet.rs:697)
pub enum DataStoreInnerSpend {
    Owner(PublicKey),
    Admin(PublicKey),
    Writer(PublicKey),
    // no Oracle variant — oracle can't change metadata/owner
}
```

`DataStore`, `DataStoreInfo`, `DataStoreMetadata`, `DelegatedPuzzle` (variants:
`Admin(TreeHash)`, `Writer(TreeHash)`, `Oracle(Bytes32, u64)`), `Proof`,
`LineageProof`, `EveProof` are **re-exported from `chia-sdk-driver` /
`chia-puzzle-types`** — not redefined.

`TargetNetwork` is **dropped** (only the excluded `sign_coin_spends` used it).

### Error type (trimmed)

`core/src/error.rs` — a `thiserror` enum carrying only the variants the copied
functions can produce: `Driver(chia_sdk_driver::DriverError)`, `Parse(String)`,
`Clvm`, `ToClvm(clvm_traits::ToClvmError)`, `Permission`. The networking,
coin-state-rejection, fee-estimate, and client variants from the original
`WalletError` are not carried.

## 4. Architecture / file layout

```
chip35_dl_coin/
  Cargo.toml                       # [workspace] members = ["core", "wasm"]
  README.md
  core/
    Cargo.toml                     # package "chip35-dl-coin"  (rlib)
    src/lib.rs                     # 5 builders + 2 serialization helpers + DataStoreInnerSpend
    src/types.rs                   # SuccessResponse
    src/error.rs                   # trimmed Error enum
    puzzles/                       # delegation_layer.clsp(.hex), writer_filter.clsp(.hex)
                                   #   + include/*.clib — REFERENCE ONLY, not compiled
  wasm/
    Cargo.toml                     # package "chip35-dl-coin-wasm"  crate-type ["cdylib","rlib"]
    src/lib.rs                     # #[wasm_bindgen] exports
    src/types.rs                   # serde boundary structs + native conversions
    scripts/patch-pkg.mjs          # rewrite pkg/package.json name/types
    types/chip35-dl-coin-wasm.d.ts # hand-authored TS types
    tests/builders.mjs             # Node test against committed fixtures
    package.json                   # build:bundler / build:node / test scripts
```

The `puzzles/` + `include/` files are carried purely as documentation of what
`chia-sdk-driver` implements (the user asked to "extract the datalayer store
puzzle"). They are **not** referenced by any Rust code and must be labeled
reference-only in the README so no one assumes they're compiled.

## 5. Dependencies (pinned to match the source → identical output)

`core/Cargo.toml`:
```toml
[dependencies]
chia-bls = "0.26.0"
chia-protocol = "0.26.0"
chia-puzzle-types = "0.26.0"
chia-puzzles = "0.20.1"
chia-traits = "0.26.0"
chia-sdk-driver = { version = "0.30.0", features = ["chip-0035", "action-layer"] }
chia-sdk-types  = { version = "0.30.0", features = ["chip-0035", "action-layer"] }
chia-sdk-utils  = "0.30.0"
clvm-traits = "0.26.0"
clvm-utils  = "0.26.0"
clvmr = "0.14.0"
thiserror = "1"
hex = "0.4"
hex-literal = "0.4"
num-bigint = "0.4"
```
`chia-sdk-signer` and `chia-consensus` are **not** expected to be needed
(signing and `get_cost` are excluded). Confirm during build; if a copied helper
transitively needs a type from either, add it back then.

`wasm/Cargo.toml`:
```toml
[lib]
crate-type = ["cdylib", "rlib"]

[features]
default = ["console-panic-hook"]
console-panic-hook = ["dep:console_error_panic_hook"]

[dependencies]
chip35-dl-coin = { path = "../core" }
wasm-bindgen = "0.2"
js-sys = "0.3"
serde = { version = "1", features = ["derive"] }
serde_bytes = "0.11"
serde-wasm-bindgen = "0.6"
console_error_panic_hook = { version = "0.1", optional = true }
hex = "0.4"
getrandom = { version = "0.2", features = ["js"] }   # see Risks — verify it's still on the wasm path
# Direct chia type access for conversions (versions match core)
chia-protocol = "0.26.0"
chia-bls = "0.26.0"
chia-sdk-driver = { version = "0.30.0", features = ["chip-0035", "action-layer"] }
chia-puzzle-types = "0.26.0"

[dev-dependencies]
wasm-bindgen-test = "0.3"
```

## 6. WASM boundary & JS interface

Same proven conventions as the source repo's WASM crate:

- camelCase JS objects via `serde-wasm-bindgen` with a serializer configured
  `serialize_large_number_types_as_bigints(true)`.
- 32/48/96-byte values ↔ `Uint8Array`; byte struct fields use
  `#[serde(with = "serde_bytes")]`.
- Amounts / `bytes` / `oracleFee` are `u64` ↔ `bigint` (exceed 2^53 — never JS
  `number`).
- `Option<T>` ↔ `T | null | undefined`.
- Errors are `Result<T, JsError>` → JS throws.
- `init()` installs `console_error_panic_hook` (idempotent).

### Exported functions (camelCase, mirroring the proven source shapes)

```ts
init(): void;

mintStore(
  minterSyntheticKey: Uint8Array, selectedCoins: Coin[], rootHash: Uint8Array,
  label: string | undefined, description: string | undefined, bytes: bigint | undefined,
  sizeProof: Uint8Array | undefined, ownerPuzzleHash: Uint8Array,
  delegatedPuzzles: DelegatedPuzzle[], fee: bigint
): SuccessResponse;

updateStoreMetadata(
  store: DataStore, newRootHash: Uint8Array, newLabel: string | undefined,
  newDescription: string | undefined, newBytes: bigint | undefined,
  newSizeProof: Uint8Array | undefined,
  ownerPublicKey?: Uint8Array, adminPublicKey?: Uint8Array, writerPublicKey?: Uint8Array
): SuccessResponse;

updateStoreOwnership(
  store: DataStore, newOwnerPuzzleHash: Uint8Array, newDelegatedPuzzles: DelegatedPuzzle[],
  ownerPublicKey?: Uint8Array, adminPublicKey?: Uint8Array
): SuccessResponse;

meltStore(store: DataStore, ownerPublicKey: Uint8Array): CoinSpend[];

oracleSpend(
  spenderSyntheticKey: Uint8Array, selectedCoins: Coin[], store: DataStore, fee: bigint
): SuccessResponse;

spendBundleToHex(spendBundle: { coinSpends: CoinSpend[]; aggregatedSignature: Uint8Array }): string;
hexSpendBundleToCoinSpends(hex: string): CoinSpend[];
```

The `update_store_*` functions flatten `DataStoreInnerSpend` into optional
`ownerPublicKey` / `adminPublicKey` / `writerPublicKey` parameters (exactly one
is supplied), mirroring the source repo's WASM signatures so existing consumers
of that interface need no shape changes.

### Boundary structs (`wasm/src/types.rs`)

`Coin`, `CoinSpend`, `LineageProof`, `EveProof`, `Proof` (oneOf:
`lineageProof?` / `eveProof?`), `DataStoreMetadata` (`sizeProof` exposed as
`Uint8Array`, stored natively as a hex string), `DelegatedPuzzle` (oneOf:
`adminInnerPuzzleHash?` / `writerInnerPuzzleHash?` / `oraclePaymentPuzzleHash?` +
`oracleFee?`), `DataStore`, `SuccessResponse` (`coinSpends`, `newStore` — reads
the native `new_datastore`). Each has `to_native`/`from_native`.

## 7. Build & packaging

- Published artifact: `wasm-pack build wasm --target bundler --release --no-opt`,
  then `node scripts/patch-pkg.mjs`. **`--no-opt` is required** — the bundled
  `wasm-opt` rejects BLST's bulk-memory ops (confirmed in the source repo).
- Test artifact: `wasm-pack build wasm --target nodejs --dev --out-dir pkg-node`.
- `patch-pkg.mjs` rewrites generated `pkg/package.json`: `name` →
  `@dignetwork/chip35-dl-coin`, `description`, `repository`, `license` (MIT),
  `types`, and copies the hand-authored `.d.ts` into `pkg/`.
- npm package name: **`@dignetwork/chip35-dl-coin`**.

## 8. Testing

1. **Node fixture test** (`wasm/tests/builders.mjs`): deterministic inputs —
   a fixed public key, fixed coins, fixed root hash — drive `mintStore`,
   `updateStoreMetadata`, `updateStoreOwnership`, `meltStore`, `oracleSpend`.
   Assert each call's `coinSpends` (and `newStore`) serialize to committed
   golden hex. Also round-trip `spendBundleToHex` →
   `hexSpendBundleToCoinSpends`. No network, no signing.
2. **Native parity guard** (Rust `#[test]` in `core/`): build the same inputs and
   compare the produced coin-spend bytes against fixtures captured from the
   original `DataLayer-Driver`. Identical by construction (same
   `chia-sdk-driver`); the test exists to catch dependency-version drift. The
   golden fixtures are generated once from the original repo and committed.

## 9. Risks & mitigations

1. **`getrandom` on wasm32** — the source repo resolved **0.2 + `js` feature**,
   no `.cargo/config.toml` needed. With signing/keygen now excluded, getrandom
   may not even reach the wasm path. Verify first build with
   `cargo tree -i getrandom --target wasm32-unknown-unknown -p chip35-dl-coin-wasm`;
   keep the `getrandom = { version = "0.2", features = ["js"] }` line only if it
   is present, otherwise drop it.
2. **`chia-sdk-driver 0.30` wasm32 compile** — already proven by the source
   repo's WASM crate; this project is a strict subset, so low risk. First
   implementation step still verifies
   `cargo build -p chip35-dl-coin --target wasm32-unknown-unknown`.
3. **Missing private helper during extraction** — the compiler names it; copy it
   in the same pass. Helpers tied only to excluded functions stay dropped.
4. **Version drift from upstream** — dependency versions are pinned; the native
   parity guard catches divergence.
5. **`bundler` output is not Node-`require`-able** — hence the separate
   `--target nodejs` build for tests; the published artifact stays `bundler`.

## 10. Out of scope / future

- Re-adding signing / coin-selection / key-derivation to the module (currently
  consumer-side by choice).
- A JS-supplied chain backend to drive sync/broadcast from WASM.
- Additional published wasm-pack targets (`web`); only `bundler` is published.
- CI/npm publish automation (this deliverable is the WASM module + tests; CI can
  be a follow-up mirroring the source repo's workflow).
