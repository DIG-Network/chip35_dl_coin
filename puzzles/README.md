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
