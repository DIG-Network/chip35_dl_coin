# DataLayer store puzzles

`delegation_layer.clsp` and `writer_filter.clsp` are the ChiaLisp puzzles that
define the CHIP-0035 DataLayer store: the singleton's delegation layer (owner /
admin / writer / oracle delegated-puzzle dispatch) and the writer-filter that
restricts a writer to metadata-only updates. The `.hex` files are their compiled
forms; `include/*.clib` are their shared library dependencies.

## How these relate to the Rust driver

The Rust driver in `core/` does **not** load or compile these files at runtime.
The compiled puzzle logic ships inside `chia-sdk-driver` (Cargo feature
`chip-0035`), which this crate depends on — the driver builds store spends via
`StandardLayer` / `WriterLayer` / `OracleLayer` from that crate. These sources
are kept here as the canonical, auditable definition of the store puzzles and
can be recompiled (see `COMPILE_PUZZLES.md`) to verify they match the `.hex`
that chia-sdk-driver uses.

## Trustless lazy-mint puzzles (reference)

`lazy_mint_pre_launcher.clsp`, `lazy_mint_direct_delegate.clsp`, and
`lazy_mint_offer_delegate.clsp` are the **secure-the-mint** puzzles for the
trustless lazy mint / mint-on-claim primitive (roadmap #40), ported from
mintgarden-io/secure-the-mint (Apache-2.0 — see `LICENSE-APACHE` + the project
`NOTICE` for full attribution; each file carries an in-file attribution header).

Like the store puzzles above, the Rust `core/src/lazy_mint.rs` builders do **not**
load these at runtime: they compose `chia-sdk-driver`'s audited `Launcher` /
`NftMint` + `P2CurriedArgs` primitives, which reproduce the same coin layout (see
`DESIGN.md` → "#40 — Trustless lazy mint" for why SDK primitives, not a custom
puzzle, back the simulator-validated path). These `.clsp` are kept as the
auditable reference of the mechanism we mirror, and to keep the door open for a
future clsp-backed payment / allowlist enforcement (both DEFERRED today). They
compile against chia's `condition_codes.clib` + `curry-and-treehash.clib` includes
(see `COMPILE_PUZZLES.md`).

### The commit → claim flow (what the builders actually do)

1. **Commit** (`build_lazy_mint_commit`): the creator's DID is spent **once**,
   emitting one `CREATE_COIN(commit_ph_i, 0)` per item. `commit_ph_i` is a
   `P2CurriedArgs` hash committing to that item's fixed "create the launcher" node.
   Because the launcher's parent is its commitment coin and the commitment coin's
   parent is the DID coin, every NFT launcher id is deterministic and is returned
   now. Afterwards the DID is never needed.
2. **Claim** (`build_lazy_mint_claim`): a non-owner spends one item's commitment
   coin (revealing its `P2Curried` node → creates the launcher), spends the launcher
   to mint the eve NFT to the recipient (free → the claimer; the offer-delegate path
   curries the payee for the deferred paid mode), and funds the 1-mojo launcher + fee
   from their own coin, asserting the commitment coin so the bundle is atomic. No DID
   involvement; provenance is by lineage (launcher ⇽ commitment coin ⇽ DID coin).

### Merkle allowlist — `merkle_utils.clib` and the proof shape

`include/merkle_utils.clib` (`simplify_merkle_proof`) is the reference clsp for the
allowlist gate. An **allowlist is a merkle tree of allowed claimer puzzle hashes**;
its root is the committed `allowlist_root`. A member's proof is `{ path, proof }` —
`path` is the LSB-first direction bits and `proof` the sibling hashes leaf→root —
hashed the standard Chia way: `sha256(0x01 || leaf)` for a leaf, `sha256(0x02 ||
left || right)` for a node, the `path` low bit selecting right(`1`)/left(`0`) at each
step. This is identical in three places by construction + test: the producer's
`chia_sdk_types::MerkleTree`, the Rust off-chain verifier
`merkle_membership_root`/`verify_merkle_membership`, and this `simplify_merkle_proof`.

The Rust builder **enforces the proof OFF-CHAIN**: a gated claim is rejected
(`ALLOWLIST_DENIED`) unless the proof proves the claimer's own puzzle hash. The
**trustless ON-CHAIN** enforcement — running `simplify_merkle_proof` inside a
compiled claim puzzle that gates the `CreateCoin` to the proven address — is
**DEFERRED**: that compiled+audited claim puzzle is not yet authored. `merkle_utils.clib`
is kept here precisely so that puzzle has a byte-compatible primitive when it lands.
