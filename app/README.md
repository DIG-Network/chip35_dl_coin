# DIG Network — CHIP-0035 store demo

A DIG Network demo UI for the `@dignetwork/chip35-dl-coin-wasm` store driver: connect a wallet, then **mint a store, advance it to a new capsule (update), and melt it (delete)** on Chia **mainnet**.

> **Store vs capsule** (DIG vocabulary): a **store** is the on-chain singleton identity; a **capsule** is one immutable generation of it (`storeId:rootHash`). Minting creates the store's first capsule; each update advances it to a new capsule. In the full DIG flow, publishing a capsule costs a small amount of **$DIG** and the content is opened with a `chia://` address — this low-level demo exercises only the underlying CHIP-0035 spends and pays the XCH network fee.

Two wallet backends, auto-selected at connect time:

- **DIG Browser** — when the page runs inside the DIG Browser its in-process wallet is injected as `window.chia` (`isDIG`). The app prefers it automatically: no QR, no relay, no project id. The button reads **Connect DIG Wallet** and approval happens in the native wallet UI.
- **WalletConnect → Sage** — outside the DIG Browser, the existing QR/relay pairing with the Sage wallet runs unchanged (needs `NEXT_PUBLIC_WALLETCONNECT_PROJECT_ID`).

Both backends return the same Sage-shaped RPC responses, so only the transport differs (`app/lib/injectedWallet.ts` vs the WalletConnect path in `app/lib/walletConnect.ts`).

> Full project docs (architecture, tests, troubleshooting) are in the [repo root README](../README.md).

## Run it

```powershell
# 1. From the repo ROOT — build the WASM package the app imports (file:../wasm/pkg)
wasm-pack build wasm --target bundler --release --no-opt

# 2. In this folder
Copy-Item .env.example .env.local      # then set NEXT_PUBLIC_WALLETCONNECT_PROJECT_ID
npm install
npm run dev                            # http://localhost:3000
```

Get a free WalletConnect/Reown project id at <https://cloud.reown.com>. `NEXT_PUBLIC_*` vars are inlined at startup — **restart `npm run dev` after editing `.env.local`**.

Static export build: `npm run build` (→ `out/`), then `npm start`.

## Scripts

| Script | Does |
|---|---|
| `npm run dev` | Next.js dev server |
| `npm run build` | Static export to `out/` |
| `npm start` | Serve `out/` on :3000 |
| `npm run lint` | Next.js lint |
| `npm test` | Unit tests (`tests/*.mjs`, Node `assert`) |

## How it loads the WASM (don't change this)

The module is loaded only through `app/lib/wasm.ts` `getWasm()` inside `dynamic(..., { ssr: false })` components — never a top-level `import`. `next.config.ts` enables `experiments.asyncWebAssembly` and routes `.wasm` output. Static `output: "export"` (no SSR) because WalletConnect's `SignClient` needs `IndexedDB` (browser-only). Breaking this pattern produces opaque build/runtime errors.

## Key files

```
app/
  next.config.ts              static export + wasm webpack config
  .env.example                NEXT_PUBLIC_WALLETCONNECT_PROJECT_ID, NEXT_PUBLIC_COINSET_BASE_URL
  app/
    page.tsx                  dashboard (dynamic, ssr:false; boots wasm via getWasm)
    layout.tsx                WalletProvider + Toaster
    lib/
      wasm.ts                 getWasm() singleton loader
      walletConnect.ts        wallet RPC: connect / getAddress / getAssetCoins / signCoinSpends
                              (routes to injectedWallet when window.chia.isDIG, else WalletConnect/Sage)
      injectedWallet.ts       DIG Browser in-process wallet adapter (window.chia)
      chiaAddress.ts          chia-wallet-sdk-wasm: address decode + uncurry → synthetic pubkey
      coinset.ts              pushTx + confirmation/liveness reads
      convert.ts              wasm ↔ WalletConnect/coinset/localStorage shapes + coin id
      registry.ts             localStorage store registry
      pendingCoins.ts         recently-spent coin guard (anti double-spend)
      storeOps.ts             mint / updateMetadata / del orchestration
    components/               WalletConnector, WalletProvider, MintForm, UpdateForm, StoreList
```

See the [root README](../README.md) troubleshooting table for the common errors (project id, stale chunks, DOUBLE_SPEND, WRONG_PUZZLE_HASH).
