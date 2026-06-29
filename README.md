# chip35_dl_coin

Isolated **CHIP-0035 Chia DataLayer store coin** driver, compiled to **WebAssembly**, with a **Next.js demo app** that lists, mints, updates, and deletes DataLayer stores using the **Sage** wallet over **WalletConnect**.

The driver was extracted from [`DataLayer-Driver`](https://github.com/DIG-Network/DataLayer-Driver) and depends only on upstream `chia-*` crates — no networking, no signing, no key derivation inside the WASM. It builds the coin spends; the consumer (the demo app) handles keys, coin selection, signing (Sage), and broadcast (coinset.org). Spend-bundle output is byte-for-byte identical to `DataLayer-Driver` (both rest on `chia-sdk-driver` 0.30 chip-0035).

## Layout

```
chip35_dl_coin/
  core/      chip35-dl-coin       — Rust driver: mint/update/ownership/melt/oracle + addFee + serialization
  wasm/      chip35-dl-coin-wasm  — wasm-bindgen bindings (the published module the app imports)
  app/       chip35-dl-coin-app   — Next.js demo UI (Sage + WalletConnect + coinset.org)
  puzzles/   delegation_layer.clsp / writer_filter.clsp — canonical store puzzle sources (reference only;
             the compiled logic ships inside chia-sdk-driver — see puzzles/README.md)
  docs/superpowers/  design spec + implementation plan
```

## What the demo does

- **Connect** the Sage wallet via WalletConnect (QR pairing).
- **Mint** a new DataLayer store (a singleton owned by your wallet key).
- **List** the stores you've created (tracked in browser localStorage) with on-chain liveness.
- **Update** a store's metadata (root hash / label / description).
- **Delete** (melt) a store.

Each mint / update / delete: builds coin spends in WASM → Sage signs (`chip0002_signCoinSpends`) → pushed to coinset.org `/push_tx` → the app waits for on-chain confirmation.

---

## Prerequisites

- **Rust** (stable) + the wasm target: `rustup target add wasm32-unknown-unknown`
- **wasm-pack**: `cargo install wasm-pack --locked`
- **LLVM/Clang** on PATH (the `blst` BLS dependency compiles C to wasm). On Windows install LLVM and ensure `C:\Program Files\LLVM\bin` is on PATH. (`wasm-pack` usually sets this up itself; only needed if a build complains about `clang`.)
- **Node.js 20+** and npm
- **Sage wallet** (desktop) set to the network you'll use (this demo targets **mainnet**)
- A free **WalletConnect / Reown project ID** from <https://cloud.reown.com>

> ⚠️ **Mainnet, real funds.** Mint/update/delete spend real XCH and pay real fees. Use small amounts. There is no automated end-to-end test of the wallet/chain flow — it is verified manually.

---

## Quick start

From the repo root (PowerShell shown; the commands are the same on bash):

```powershell
# 1. Build the WASM package → wasm/pkg  (the app depends on it via file:../wasm/pkg)
wasm-pack build wasm --target bundler --release --no-opt
#   (equivalent: cd wasm; npm run build:bundler; cd ..)

# 2. Configure the app
cd app
Copy-Item .env.example .env.local
#   then edit .env.local and set your project id:
#   NEXT_PUBLIC_WALLETCONNECT_PROJECT_ID=<your reown project id>
npm install

# 3. Run the dev server
npm run dev
#   open http://localhost:3000
```

`--no-opt` is required: the bundled `wasm-opt` rejects BLST's bulk-memory ops. The crate also sets `wasm-opt = false` in `wasm/Cargo.toml`, so a plain `wasm-pack build wasm --target bundler --release` works too.

### `app/.env.local`

```dotenv
# Same project id you'd use for any Sage/WalletConnect dapp (https://cloud.reown.com)
NEXT_PUBLIC_WALLETCONNECT_PROJECT_ID=your_reown_project_id
# coinset.org RPC (reads + /push_tx). Default is fine.
NEXT_PUBLIC_COINSET_BASE_URL=https://api.coinset.org
```

`NEXT_PUBLIC_*` values are inlined when the dev server (or build) **starts** — **restart `npm run dev` after editing `.env.local`**.

### Production (static export)

```powershell
cd app
npm run build      # → app/out  (Next.js static export)
npm start          # serves app/out on http://localhost:3000
```

---

## Using the demo

1. Open <http://localhost:3000>. Make sure Sage is running and set to **mainnet**, with a small spendable XCH balance.
2. Click **Connect Wallet**, scan the QR with Sage, approve.
3. **Mint:** set a label/description, a 32-byte root hash (or click *random*), and a fee (default `1,000,000` mojos). Submit, approve in Sage. Watch the status: *Pushing → waiting for on-chain confirmation → Confirmed*.
4. **Update / Delete:** pick a store from the list, set the new root hash (update) or confirm the fee (delete), approve in Sage, wait for confirmation.

The store list lives in your browser's localStorage; "Refresh status" checks each store's current coin on coinset.org.

---

## Tests

```powershell
# Rust driver unit + parity tests
cargo test -p chip35-dl-coin

# WASM functional test (builds the nodejs target, runs mint/update/melt/oracle + native↔wasm golden parity)
cd wasm; npm test; cd ..

# App typecheck / build
cd app; npx tsc --noEmit; npm run build; cd ..
```

---

## How it works (architecture)

- **`chip35-dl-coin-wasm`** exposes:
  - **DataStore spend builders:** `mintStore`, `updateStoreMetadata`, `updateStoreOwnership`, `meltStore`, `oracleSpend`, plus `addFee`, `digstoreOwnerHint`, `dataStoreFromSpend`, and `spendBundleToHex` / `hexSpendBundleToCoinSpends`.
  - **DataStore delegation (hub Teams #43 + revocable deploy tokens #17):** `adminDelegatedPuzzleFromKey`, `writerDelegatedPuzzleFromKey`, `oracleDelegatedPuzzle` — build the delegate entry to add to a store's delegated-puzzle set (via `mintStore`/`updateStoreOwnership`). An **admin** updates the store + changes delegation; a **writer** advances the root (deploy) but cannot change delegation — a **deploy token is a revocable writer key**; an **oracle** lets anyone spend for a fixed fee. The delegate advances the root via `updateStoreMetadata` with the `writerPublicKey`/`adminPublicKey` argument (no owner seed).
  - **Asset toolkit (roadmap #33/#34/#35/#36):** `mintNft` (NFT with dig:// + https-fallback media URIs and computed hashes), `bulkMint` + `generateItemMetadata` (collection bulk mint from a parsed traits manifest), `createDid` (creator identity), `issueCat` (fixed-supply CAT), `encodeOffer` / `decodeOffer` (offer codec), and the CHIP-0007 helpers `buildChip0007Metadata`, `validateChip0007`, `sha256`.
  - **In-dapp monetization (roadmap #46) — a dapp deployed on DIG can EARN:** `buildPayment` / `buildCatPayment` (a buyer pays the dapp owner in XCH or any CAT incl. DIG, settling to the owner's address), `verifyPaymentReceipt` (paywall / pay-to-unlock: verify a confirmed payment of ≥ amount to the owner, with a replay-proof nonce) + `paymentNonce`, and the NFT-gating reads `proveNftOwnership` / `proveCollectionMembership` / `readNftOwnership` (prove a wallet holds an NFT / a collection member — read/verify, not a spend). Recurring **subscriptions** are scaffolded (clear TODO; need a time-locked/delegated puzzle) — until then model recurring billing as one payment per period.
  - **Runtime self-description (agent-friendly):** `version()` returns the package version string (= the npm package version), and `capabilities()` returns a machine-readable descriptor `{ name, version, builders, errorCodes }` — the full builder catalogue + the stable error-code list — so a consumer or agent can introspect the loaded surface at runtime with zero out-of-band knowledge.

  It returns `CoinSpend[]` (and a result summary such as the updated `DataStore`, the minted NFT/DID launcher id, or the CAT asset id) — it never signs, derives keys, or touches the network. See `DESIGN.md` for the full asset-toolkit + delegation design.

### Typed exports + the error-code contract (agent-friendly)

- **Real TypeScript types.** The published `.d.ts` types every builder's inputs/outputs with concrete interfaces (`Coin`, `CoinSpend`, `DataStore`, `SuccessResponse`, `NftMintParams`, `PaymentReceipt`, `NftOwnershipProof`, `Capabilities`, …) instead of `any`, including discriminated unions for the "exactly one of" shapes: `Proof` (`lineageProof` | `eveProof`), `DelegatedPuzzle` (admin | writer | oracle), and `PaymentAsset` (`{ xch:true }` | `{ assetId }`). Encoding rules: 32/48/96-byte hashes/keys/signatures are `Uint8Array` (raw bytes, not hex); `u64`/amounts are `bigint`; keys are `camelCase`.
- **Stable machine error codes.** Every failure carries a stable `UPPER_SNAKE` code an automated caller branches on — never parse the human message. A **throwing** export rejects with a structured `ChipError` object `{ code, message }`; the **result-shaped** helpers carry the same `code` as a field: `verifyPaymentReceipt` → `{ ok, code?, error? }` (a `PaywallError` code on denial), and the gating reads → `{ ok, proof?, code?, error? }` (a `GatingError` code on failure). The full catalogue (`ChipErrorCode`) is in the `.d.ts` and discoverable at runtime via `capabilities().errorCodes`:

  | Code | Where it comes from |
  |---|---|
  | `INVALID_ARGUMENT` | a wasm argument is the wrong length/shape (bad key, non-32-byte hash, missing "exactly one of" selector) |
  | `SERDE_ERROR` | a JS value failed to (de)serialize at the boundary |
  | `DRIVER_ERROR` | the underlying chia driver failed to construct the spend |
  | `PARSE_ERROR` | the builder rejected its inputs (e.g. empty coin selection, insufficient funds) |
  | `PERMISSION_DENIED` | the puzzle can't perform the requested action |
  | `METADATA_ERROR` | CHIP-0007 metadata failed schema validation / serialization |
  | `NOT_AN_NFT` · `WRONG_OWNER` · `WRONG_COLLECTION` · `WRONG_NFT` | NFT-gating denial (`GatingError`) |
  | `WRONG_RECIPIENT` · `INSUFFICIENT_AMOUNT` · `WRONG_ASSET` · `NONCE_MISMATCH` | paywall denial (`PaywallError`) |
- The app uses **`chia-wallet-sdk-wasm`** for wallet utilities the driver omits: bech32m address decode, and uncurrying a coin's puzzle reveal to recover its synthetic public key.
- **Sage** (over WalletConnect) provides spendable coins (`chip0002_getAssetCoins`) and signs (`chip0002_signCoinSpends`, `partial:false, auto_submit:false`).
- **coinset.org** broadcasts (`/push_tx`) and provides confirmation/liveness reads.
- A store is owned by the **synthetic key of the coin that funded its mint** (the key Sage signs with), so its whole mint → update → melt lifecycle stays self-consistent.

---

## Limitations / notes

- **Demo registry is local.** "List" shows stores minted/updated in *this* browser (localStorage). Importing arbitrary pre-existing stores by launcher id is out of scope.
- **Update/delete assume this app is the sole spender** of a tracked store (its cached `DataStore` is the latest state).
- **WASM import discipline:** the module is only loaded via `app/app/lib/wasm.ts` `getWasm()` inside `dynamic(..., { ssr: false })` components — never a top-level import. `next.config.ts` enables `asyncWebAssembly`. Don't change this pattern or the build breaks.

---

## Troubleshooting

| Symptom | Cause / fix |
|---|---|
| `WalletConnect client could not be initialised. Check NEXT_PUBLIC_WALLETCONNECT_PROJECT_ID` | `.env.local` missing the project id, or the dev server wasn't restarted after adding it. Set the id and restart `npm run dev`. |
| `Loading chunk … failed` | Stale dev-server chunks after source changed under a running `npm run dev`. Restart the dev server and hard-refresh (Ctrl+Shift+R). |
| `/push_tx rejected: … DOUBLE_SPEND` | The selected coin was already spent / still in the mempool. The app marks it and you can retry to pick a different coin; or wait for a prior tx to confirm. |
| `/push_tx rejected: … WRONG_PUZZLE_HASH` | A store minted by an **older build** is owned by a key the app didn't store. Clear the old list (`localStorage.clear()` in the console) and **mint a fresh store** with the current build. |
| `No spendable XCH coin found covering N mojos` | The wallet has no single unlocked coin ≥ fee+1 (mint) or ≥ fee (update/delete). Fund the wallet or lower the fee. |
| `next build` fails with `EPERM … .next\trace` | A `npm run dev` is already running and locking `.next`. Stop it before `npm run build`. |

---

## License

MIT. See `LICENSE` (driver code extracted from DataLayer-Driver, also MIT).
