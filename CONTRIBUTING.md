# Contributing to chip35_dl_coin

Thanks for your interest in improving this repo. It has two very different halves — an
offline Chia coin-spend driver compiled to WebAssembly, and a Next.js demo app that
exercises it against a real Sage wallet on mainnet — so read the prerequisites for
whichever half you're touching.

## What this repo is

An isolated **CHIP-0035 Chia DataLayer store coin** driver, compiled to **WebAssembly**,
plus a **Next.js demo app** that lists, mints, updates, and deletes DataLayer stores
using the **Sage** wallet over **WalletConnect**. The driver builds coin spends only; it
does no networking, signing, or key derivation.

## Reporting an issue

File it at <https://github.com/DIG-Network/chip35_dl_coin/issues> with what you
observed, what you expected, and steps to reproduce (a failing `cargo test` or a
mint/update/melt flow in the demo app, if applicable).

## Prerequisites

- **Rust** (stable — there is no `rust-toolchain.toml` pin in this repo) plus the wasm
  target: `rustup target add wasm32-unknown-unknown`.
- **wasm-pack**: install via the official installer
  (`curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh`, or
  `cargo install wasm-pack --locked`).
- **LLVM/Clang** on `PATH` — the `blst` BLS dependency compiles C to wasm. On Windows,
  install LLVM and make sure `C:\Program Files\LLVM\bin` is on `PATH`; `wasm-pack`
  usually sets this up itself and this is only needed if a build complains about
  `clang`.
- **Node.js 20+** and npm, for the wasm package's own test harness and for the demo app.
- Only if you're touching the demo app: **Sage wallet** (desktop) on mainnet, and a free
  WalletConnect/Reown project id from <https://cloud.reown.com>.

### Build order — the wasm package must exist before the demo app can install

`app/package.json` depends on the wasm bindings via a local path,
`"chip35-dl-coin-wasm": "file:../wasm/pkg"` — not the published npm package — so
`wasm/pkg/` must be built before `npm install` in `app/` will resolve:

```sh
# from the repo root
wasm-pack build wasm --target bundler --release --no-opt
node wasm/scripts/patch-pkg.mjs   # rewrites pkg/package.json to the scoped @dignetwork name

# then, in app/
cp .env.example .env.local        # set NEXT_PUBLIC_WALLETCONNECT_PROJECT_ID
npm install
npm run dev                       # http://localhost:3000
```

`wasm/package.json`'s `build:bundler` script runs both of those first two steps for you
(`npm run build:bundler` from `wasm/`).

## Build & test

Rust driver (`core/`) and its wasm bindings (`wasm/`) are one Cargo workspace at the
repo root:

```sh
# build both workspace members
cargo build --workspace

# run the core driver's test suite (includes the chia-sdk-test Simulator validation gate)
cargo test -p chip35-dl-coin
```

The wasm crate's own build + JS-shape test (builds a Node-target package and runs
`tests/builders.mjs` against it):

```sh
cd wasm
npm test
```

The demo app has its own lightweight test script (`app/tests/injectedWallet.mjs`, run via
`npm test` from `app/`), but it is not part of the CI gate below.

## The gate (must pass before a PR is merged)

CI runs these on every PR (`.github/workflows/ci.yml`); run them locally first:

```sh
# format
cargo fmt --all -- --check

# lint (core, then wasm target)
cargo clippy -p chip35-dl-coin --all-targets -- -D warnings
cargo clippy -p chip35-dl-coin-wasm --target wasm32-unknown-unknown -- -D warnings

# tests (core, via nextest — retries flaky tests up to 2x per .config/nextest.toml)
cargo nextest run -p chip35-dl-coin

# coverage gate: core must stay >=90% lines / >=85% regions
cargo llvm-cov nextest -p chip35-dl-coin --fail-under-lines 90 --fail-under-regions 85

# wasm build + JS-shape test (from wasm/)
npm test
```

The Next.js demo app (`app/`) is not built or linted in CI — it is a manual demo
surface, not a gated package.

## Version bumps and keeping files in sync

This repo is a Cargo workspace with **no version at the repo root** (`Cargo.toml` at
the root is `[workspace]`-only). The actually-published unit is the **wasm crate**:
`wasm/Cargo.toml`'s `version` flows through `wasm-pack` into the published npm package
`@dignetwork/chip35-dl-coin-wasm`. `.github/workflows/ensure-version-increment.yml`
enforces this precisely:

- it reads `wasm/Cargo.toml`'s version (present) and a repo-root `package.json`'s
  version (currently **absent** from this repo, so that half of the check is a no-op);
- if both existed they would have to increase versus `main` **and equal each other**;
  today, only `wasm/Cargo.toml`'s version must strictly increase versus `main`.
- `core/Cargo.toml` (the driver crate, published separately to crates.io via
  `publish-crate.yml` off a `core-v*` tag) is **not** read by this gate and can version
  independently of `wasm/Cargo.toml`.

So: bump `wasm/Cargo.toml`'s `version` on every PR that changes anything under `wasm/`
(and any PR affecting the published npm package's behavior). Bump `core/Cargo.toml`'s
version too if you changed `core/`, matching the SemVer rule (patch/minor/major) to
what the driver crate needs on crates.io — the two versions do not need to match each
other.

## Pull requests

`main` is protected: branch off it, open a PR, and squash-merge only once every CI
check is green and every review thread (including any CodeQL/GHAS finding) is
resolved.

- Commit messages and the PR title follow [Conventional
  Commits](https://www.conventionalcommits.org) (`type(scope): summary`), enforced in
  CI by `.github/workflows/commitlint.yml` against `commitlint.config.mjs`. Allowed
  types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`,
  `chore`, `revert`. A breaking change appends `!` and/or a `BREAKING CHANGE:` footer.
- On merge, `.github/workflows/release.yml` regenerates `CHANGELOG.md` from those
  commits (git-cliff) and tags the result `vX.Y.Z` (from `wasm/Cargo.toml`'s version),
  which fires `publish-npm.yml`. Publishing the core crate to crates.io is separate:
  a manual dispatch or a pushed `core-v*` tag runs `publish-crate.yml`.
- Keep the diff focused, and update `README.md`/`SPEC.md`/`DESIGN.md` in the same PR
  if your change makes any of them stale.
