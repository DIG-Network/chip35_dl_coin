# chip35-dl-coin — normative specification

This document is the authoritative contract for the `chip35-dl-coin` crate and its
`chip35-dl-coin-wasm` bindings. It states what the crate IS and MUST do, at the level an
independent reimplementation could be built and checked against. It is not a tutorial and not a
changelog.

Keywords MUST, MUST NOT, SHOULD and MAY are used in their normative sense.

---

## 1. Scope and boundary

`chip35-dl-coin` is an **offline, keyless spend-bundle builder** for CHIP-0035 Chia DataLayer store
coins and the DIG asset primitives layered on them.

The crate MUST NOT, in any code path:

- perform network I/O of any kind,
- hold, derive, read or import a private key or seed,
- produce a signature or an aggregate signature,
- select coins from a wallet, or query a coin's on-chain state.

Every builder is a pure function of its arguments. It returns **unsigned** `CoinSpend`s. The caller
owns key management, coin selection, signing and broadcast. This boundary is the crate's central
security property: a consumer can run it in a browser, in a node process, or inside an untrusted
sandbox without exposing key material, because there is none to expose.

Two consequences follow, and both are normative:

1. Builders MUST be **deterministic**. The same arguments MUST produce byte-identical output, in the
   same order, on every platform and in every process. Nothing may depend on wall-clock time,
   iteration order of an unordered collection, address-space layout, or a random source.
2. Builders MUST NOT silently substitute a value the caller did not supply. In particular the
   per-capsule $DIG price is an **input**, never a constant read from anywhere (§4.2).

---

## 2. Determinism and the golden vectors

The bytes this crate produces are what a user ultimately signs and what the chain admits. A change
to those bytes is a change to what a user's key authorizes, whether or not any public signature
changed.

`core/tests/golden_vectors.rs` therefore pins, for each of the crate's principal entry points, the
**sha256 of the canonically serialized `SpendBundle`** together with the per-coin-spend coin id,
puzzle hash and amount. The pinned entry points are:

| vector | entry point |
|---|---|
| `golden_mint_store` | `mint_store` — the DataLayer store singleton launch |
| `golden_dig_store_payment` | `build_dig_store_payment` — the per-capsule $DIG payment |
| `golden_xch_payment` | `build_xch_payment` — the XCH transfer |
| `golden_create_did` | `create_did` — the DID singleton launch |
| `golden_lazy_mint_commit` | `build_lazy_mint_commit` — the creator precommit |
| `golden_lazy_mint_claim` | `build_lazy_mint_claim` — mint-on-claim |
| `golden_issue_cat` | `issue_cat` — CAT eve issuance (the only TAIL reveal) |

**A failing golden vector MUST NOT be resolved by updating the expected value.** A changed vector
means the crate now signs something different than the released version did. That is a decision to
be recorded — in this document and in the release notes — and never an adjustment made to restore a
green build. A dependency migration in particular is correct exactly when every vector still holds:
the vectors are the instrument that distinguishes a version bump from a behaviour change.

New public builders SHOULD arrive with a vector. A builder with no vector is covered only for shape,
and shape is invariant under precisely the changes that are dangerous here.

---

## 3. Store coin lifecycle

The store coin is a Chia singleton whose inner puzzle is the CHIP-0035 delegation layer.

- `mint_store` launches the singleton from a lead coin, attaches the initial metadata
  (`root_hash`, and the optional `label`, `description`, `bytes`, `program_hash`), sets the owner
  puzzle hash, installs the delegated-puzzle set, reserves the fee, and returns change to the
  minter. Minting MUST NOT require or spend $DIG (§4.1).
- `update_store_metadata` advances the root. A root advance IS a metadata update; there is no
  separate operation for it.
- `update_store_ownership` replaces the owner and/or the delegated-puzzle set. Revocation of a
  delegate is performed by replacing the set without that delegate — there is no revoke primitive.
- `melt_store` burns the singleton.
- `oracle_spend` performs the fee-paying oracle spend permitted by the delegation layer.

Delegated puzzles are `DelegatedPuzzle::{Admin, Writer, Oracle}`. An **admin** may change the
delegated-puzzle set; a **writer** may advance the root but MUST NOT change ownership. A deploy
token is not a distinct type: it is a writer delegate for a CI key, issued by adding
`writer_delegated_puzzle_from_key` to the set and revoked by removing it.

Where multiple coins fund a spend, the **first** element of `selected_coins` is the lead coin. Every
other coin MUST be spent with an `ASSERT_CONCURRENT_SPEND` on the lead coin's id, so the group is
atomic. The lead coin carries the launch conditions, the fee reservation and the change output.

An empty `selected_coins` MUST return `WalletError::Parse`, never panic.

---

## 4. $DIG economics

### 4.1 Minting is free; a capsule is paid

Creating a store MUST NOT cost $DIG. $DIG is paid when a **capsule** is created — every commit /
root advance producing a new `(storeId, rootHash)` generation. The commit path MUST concatenate the
payment coin spends with the root-advance singleton spend into ONE bundle, signed together, so the
payment and the capsule are admitted atomically or not at all.

### 4.2 The price is an input

The per-capsule price is dynamic and USD-pegged (`amount = target_usd ÷ live DIG price`). This crate
is offline and MUST NOT fetch a price. `amount`, in DIG base units, is a parameter. There is
deliberately no price constant in this crate.

### 4.3 Byte-identical shared contract

The following are a cross-repo shared contract and MUST be byte-identical everywhere they appear
(`SYSTEM.md` → Shared contracts → "DIG CAT payment"):

| value | source |
|---|---|
| DIG CAT asset id | re-exported from `dig-constants::DIG_ASSET_ID` |
| DIG treasury inner puzzle hash | `ec7c304708c7d59c078d5ae098d0dea004decf47fa1cafebb266c10ad6466ce8` |
| treasury output memos | `[treasury_inner_puzzle_hash (hint), store_id]`, in that order |

`build_dig_store_payment` MUST reject a CAT set that is empty, mixes asset ids, is not the DIG
asset, or does not cover `amount`. It ring-spends the supplied CATs, creates one treasury output
carrying the memos above, and hints the change back to the buyer's own inner puzzle hash. It
reserves no XCH; the network fee rides on the commit's own spend.

`dig_treasury_payment_coin` MUST return exactly the coin `build_dig_store_payment` creates, so a
caller can pin the expected payment coin without re-deriving the CAT wrap.

### 4.4 General payments

`build_xch_payment` and `build_cat_payment` transfer to an arbitrary puzzle hash and attach memos
`[recipient_puzzle_hash, nonce]`. The `nonce` exists so `verify_payment_receipt` can match an
observed on-chain coin to an expected payment; it is part of the signed bytes and is therefore
visible on chain. A caller building an ordinary user-initiated transfer, rather than settling a
specific invoice, SHOULD be aware that this memo asserts a settlement relationship.

`verify_payment_receipt` MUST confirm asset, amount, recipient and nonce against the observed coin.
It MUST NOT infer a payment from amount alone.

---

## 5. Errors

All fallible entry points return `Result<_, WalletError>`.

- `WalletError::Parse` — the caller's arguments are invalid (empty coin set, mixed asset ids,
  insufficient value, an out-of-range index).
- `WalletError::Driver` — spend construction failed inside the driver layer.

Builders MUST NOT panic on caller-supplied input. Validation MUST happen before any spend is
constructed, so a rejected call has no partial effect.

---

## 6. Build requirements

### 6.1 Dependency line

The crate tracks the **chia 0.36.1 / chia-sdk 0.36** line, with `chia-sdk-driver` and
`chia-sdk-types` built with the `chip-0035`, `action-layer` and `offer-compression` features.
`chia-sdk` 0.36 is the highest published release of that family; the `chia-*` primitives publish
beyond 0.36.1 but are unreachable from it, so 0.36 is a ceiling rather than a lag.

Spend-bundle output remains byte-for-byte identical to `DataLayer-Driver`: the CHIP-0035 delegation,
writer and oracle layers and the store launcher are unchanged across `chia-sdk` 0.34 → 0.36, and the
golden vectors in `core/tests/golden_vectors.rs` reproduce unmodified. Rust *type identity* is a
separate matter — a `SpendBundle` composes with another DIG spend builder's only when both are on
this same line, so every crate whose bundles are concatenated with these MUST track it.

The crate MUST NOT vendor or `[patch]` a chia crate. crates.io strips patch and vendored
dependencies, so such a crate publishes green and ships broken.

### 6.2 wasm32 consumers

`chia-sdk` 0.36 reaches `getrandom` 0.3, which has **no default backend for
`wasm32-unknown-unknown`**. This crate selects the browser backend on the consumer's behalf, as a
target-gated dependency in `core/Cargo.toml`:

```toml
[target.wasm32-unknown-unknown.dependencies]
getrandom = { version = "0.3", features = ["wasm_js"] }
```

Because cargo unifies features across the dependency graph, a downstream Rust crate that depends on
`chip35-dl-coin` and targets `wasm32-unknown-unknown` inherits this and needs **no build flag of its
own**. This is deliberately not a `.cargo/config.toml`: cargo does not publish that file with the
crate, so a config-based fix would work in this repository and fail for every consumer.

`getrandom`'s own compile error states that the `wasm_js` feature alone is insufficient and a
`getrandom_backend="wasm_js"` cfg is also required. As of getrandom **0.3.4** that text is stale —
`src/backends.rs` selects the backend on the bare feature when the target is wasm32 and no explicit
backend cfg is set, verified by building this crate for that target with the feature and no cfg.
Should a future getrandom re-tighten this, the cfg belongs in the **consuming binary's**
`.cargo/config.toml`:

```toml
[target.wasm32-unknown-unknown]
rustflags = ['--cfg', 'getrandom_backend="wasm_js"']
```

---

## 7. Versioning and compatibility

The crate follows SemVer, on a `0.x` line where a breaking change is a **minor** bump.

The following are breaking and MUST bump accordingly:

- removing or renaming a public item, or changing a public signature;
- moving to a different chia / chia-sdk crate line, because the re-exported chia types
  (`SpendBundle`, `CoinSpend`, `Bytes32`, `Cat`, …) are part of this crate's public surface and do
  not unify across lines;
- any change to the bytes a builder produces (which the golden vectors exist to detect).

The two publishable units version independently:

| unit | registry | version source | release trigger |
|---|---|---|---|
| `chip35-dl-coin` | crates.io | `core/Cargo.toml` | `workflow_dispatch`, or a pushed `core-v*` tag |
| `chip35-dl-coin-wasm` | npm | `wasm/Cargo.toml` | the `v*` tag cut by `release.yml` |

`release.yml` and the version-increment gate both read `wasm/Cargo.toml`. The core crate's version
is therefore **outside** the automatic tag path and is published deliberately.

Consumers MUST depend on the crates.io version. A `git = …` dependency on this crate is forbidden by
the ecosystem no-git-deps policy and cannot be published against.
