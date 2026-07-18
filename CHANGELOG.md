# Changelog

All notable changes to this project are documented here.
This project adheres to [Semantic Versioning](https://semver.org) and
[Conventional Commits](https://www.conventionalcommits.org).

## [0.14.4] - 2026-07-18

### Refactor
- **chip35:** Consume DIG_ASSET_ID from dig-constants (#971) (#8)

## [0.14.3] - 2026-07-18

### CI
- **chip35:** Publish the core chip35-dl-coin crate to crates.io (#7)

## [0.14.2] - 2026-07-17

### Bug Fixes
- **nft:** Emit bare root-pinned URN for NFT media URIs (drop dig:// prefix) (#6)

## [0.14.1] - 2026-07-12

### CI
- Add flaky-test management (#489) (#5)

## [0.14.0] - 2026-07-11

### Features
- **select:** Shared selectCoins (cap 50, NeedsConsolidation) + coin/CAT consolidation builders (#4)

## [0.13.0] - 2026-07-11

### Features
- **collection:** Fund multi-item bulk mints with a separate XCH coin (#3)

## [0.12.1] - 2026-07-07

### CI
- Publish via npm trusted publishing (OIDC), retire NPM_TOKEN (#2)

## [0.12.0] - 2026-07-07

### Features
- **metadata:** Emit CHIP-0007 collection attributes as "type", accept legacy "trait_type" on read (#1)

### CI
- Enforce version increment in PRs (package.json / Cargo.toml)- Enforce Conventional Commits with commitlint on PRs- Enforce Conventional Commits with commitlint on PRs- Auto-publish npm on version tag + changelog/tag on merge (#230 auto-publish-everything)

### Chores
- **changelog:** Add git-cliff config for Conventional-Commit changelog

### README
- Document the coverage command + the CI coverage gate

## [0.8.0] - 2026-06-29

### Features
- **core:** Scaffold chip35-dl-coin crate (error, types, module wiring)- **core:** DataLayer store spend builders (mint/update/ownership/melt/oracle) + serialization- Add canonical DataLayer store puzzle sources (delegation_layer, writer_filter)- **wasm:** Scaffold chip35-dl-coin-wasm crate with init()- **app:** Next.js scaffold + wasm loader + WalletConnect/coinset/registry/storeOps plumbing- **app:** Demo UI — connect (QR), mint, list+liveness, update, delete DataLayer stores- **app:** Wait for coinset.org confirmation on mint/update/delete + status UI- **core,wasm:** Add addFee builder for attaching fees to singleton-only spends- **core,wasm:** Store program_hash in the DataStore metadata slot (was size_proof)- **app:** Program hash field + inputs (was size proof)- Digstore-scoped owner discovery hint at mint (0.2.0)- DataStoreFromSpend — reconstruct a DataStore for melt (0.3.0)- **app:** Use DIG Browser window.chia as a WalletConnect alternative- Asset toolkit spend builders + CHIP-0007 metadata + deploy-token scaffold- DataStore delegation builders (hub Teams #43 + deploy tokens #17)- **core:** Stable UPPER_SNAKE error codes on Error/GatingError/PaywallError- **wasm:** Version()/capabilities(), typed .d.ts exports, structured errors (0.8.0)

### Bug Fixes
- **core,wasm:** Guard empty coin slice; native<->wasm golden parity + oracle/ownership test coverage- **app:** Validate Sage signature shape; simplify push_tx; trim WC namespace; drop dead coinId branch- **app:** Robust coin selection (avoid double-spend) + attach fees to update/delete via addFee + fee inputs- **app:** Derive store owner from funding-coin key, not wallet address (fixes WRONG_PUZZLE_HASH on melt/update)- **app:** Display Program Hash row in the store list- **app:** Prefill program hash on edit (preserve current) + Random button in UpdateForm- **wasm:** Publish scoped @dignetwork/chip35-dl-coin-wasm; bump 0.5.0

### Refactor
- **core:** Drop unused deps + dead error variant; add error context and permission-denied tests

### Documentation
- Add root + app run-the-demo READMEs (build wasm, configure env, run, troubleshoot)- Rustdoc the public spend-builder API; refresh stale mint fixture

### Testing
- **core:** Deterministic fixtures generator for node parity- **wasm:** Node builder test (mint/update/melt + serialization + determinism)

### CI
- Add npm publish workflow- Install wasm-pack via official installer

### Chores
- Init chip35_dl_coin workspace + spec/plan- Commit Cargo.lock to pin reproducible spend-bundle output- **app:** Retitle header to 'CHIP-0035 DataLayer Tech Demo'- **wasm:** 0.6.0 — DataStore delegation builders (admin/writer/oracle) for Teams #43 + deploy tokens #17- **wasm:** 0.7.0 — in-dapp monetization spends (payment/paywall/NFT-gating) for #46

### Core
- Doc owner-hint as capsule-lineage owner discovery (doc-only)


