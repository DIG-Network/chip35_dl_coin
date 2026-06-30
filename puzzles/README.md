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
