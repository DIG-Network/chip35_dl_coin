# chip35-dl-coin-app

Next.js demo UI for the CHIP-0035 DataLayer store driver: connect the **Sage** wallet over **WalletConnect**, then **list / mint / update / delete** DataLayer stores on Chia **mainnet**.

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
      walletConnect.ts        SignClient: connect / getAddress / getAssetCoins / signCoinSpends
      chiaAddress.ts          chia-wallet-sdk-wasm: address decode + uncurry → synthetic pubkey
      coinset.ts              pushTx + confirmation/liveness reads
      convert.ts              wasm ↔ WalletConnect/coinset/localStorage shapes + coin id
      registry.ts             localStorage store registry
      pendingCoins.ts         recently-spent coin guard (anti double-spend)
      storeOps.ts             mint / updateMetadata / del orchestration
    components/               WalletConnector, WalletProvider, MintForm, UpdateForm, StoreList
```

See the [root README](../README.md) troubleshooting table for the common errors (project id, stale chunks, DOUBLE_SPEND, WRONG_PUZZLE_HASH).
