// ============================================================================
// wasm.ts — lazy WASM loader for `chip35-dl-coin-wasm`
// ============================================================================
//
// MODULE: lib/wasm
// PURPOSE: Centralise the dynamic-import pattern that loads the
//          `chip35-dl-coin-wasm` package. NEVER static-import this
//          package at the module top-level — the wasm glue requires
//          a browser environment with WebAssembly support.
//
// WHY LAZY: Next.js prerenders pages on the server. Importing the
// wasm package at module-top-level crashes the prerender pass with
// "WebAssembly.instantiate" / "ReferenceError: window is not
// defined" depending on the bundling stage. A `'use client'`
// directive plus a dynamic `await import(...)` inside an effect
// (or inside a `dynamic(..., {ssr: false})` factory) is the
// supported workflow.
//
// USAGE FROM A COMPONENT (preferred):
//
//   export default dynamic(
//     async function DynamicElem() {
//       const wasm = await getWasm();
//       return function MyPage() { /* ... */ };
//     },
//     { ssr: false }
//   );

let cached: typeof import("chip35-dl-coin-wasm") | null = null;
let loading: Promise<typeof import("chip35-dl-coin-wasm")> | null = null;

/**
 * Lazily load (or return the cached) `chip35-dl-coin-wasm` module.
 * Safe to call from anywhere on the client; do NOT call server-side.
 * You should be inside a `'use client'` component or handler.
 */
export async function getWasm(): Promise<typeof import("chip35-dl-coin-wasm")> {
  if (cached) return cached;
  if (loading) return loading;
  loading = (async () => {
    const wasm = await import("chip35-dl-coin-wasm");
    // Handle both `--target web` (async default export) and
    // `--target bundler` (no default export — wasm is pre-instantiated
    // by the glue module at import time).
    const d = wasm as { default?: unknown };
    if (typeof d.default === "function") {
      await (d.default as () => Promise<unknown>)();
    }
    wasm.init();
    cached = wasm;
    return wasm;
  })();
  return loading;
}

/** Convenience type alias for the wasm module. */
export type WasmModule = typeof import("chip35-dl-coin-wasm");
