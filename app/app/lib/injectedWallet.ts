// ============================================================================
// injectedWallet.ts — DIG Browser in-process wallet backend (window.chia)
// ============================================================================
//
// MODULE: lib/injectedWallet
// PURPOSE: Thin, dependency-free adapter over the DIG Browser's injected wallet
//          provider, exposed on every page as `window.chia`. When that provider
//          is present we PREFER it over WalletConnect: no QR, no relay, no
//          pairing. The native provider returns the SAME response shapes Sage
//          returns over WalletConnect, so walletConnect.ts's existing call sites
//          and parsing are unchanged — this module only swaps the TRANSPORT.
//
// This mirrors hub.dig.net's lib/injected-wallet.js (the canonical pattern):
//   window.chia = { isDIG, isConnected, request({method,params}),
//                   connect(eager), on(evt,fn), off(evt,fn) }
//   • request({method, params}) POSTs to the in-process wallet, handles
//     202-pending (awaits user approval + polls), and resolves to the wallet's
//     `data` — the same shape WalletConnect's request() resolves to.
//   • connect(eager) blocks until the user approves THIS origin in the native
//     wallet UI (per-origin consent on the unspoofable Origin header).
//
// BROWSER-ONLY: every path guards on `typeof window` so this is safe during
// Next.js's server prerender pass.

/** The injected provider's request envelope. */
interface InjectedRequestArgs {
  method: string;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  params?: any;
}

/** Minimal shape of the DIG Browser's injected `window.chia` provider. */
export interface InjectedChiaProvider {
  /** Explicit, unspoofable marker that this is the DIG Browser wallet. */
  isDIG?: boolean;
  isConnected?: boolean;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  request(args: InjectedRequestArgs): Promise<any>;
  connect?(eager?: boolean): Promise<unknown>;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  on?(event: string, handler: (...args: any[]) => void): void;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  off?(event: string, handler: (...args: any[]) => void): void;
}

declare global {
  interface Window {
    chia?: InjectedChiaProvider;
  }
}

/**
 * The CHIP-0002 / chia method set the chip35 demo uses (mirrors the methods
 * walletConnect.ts requests). A method NOT in this set is rejected up front so
 * callers surface an actionable error instead of firing a request the native
 * wallet can't answer.
 */
export const INJECTED_METHODS = [
  "chia_getAddress",
  "chip0002_getAssetCoins",
  "chip0002_signCoinSpends",
] as const;

/**
 * The injected provider object, or undefined when not running inside the DIG
 * Browser. Guarded for SSR / Next.js prerender (no `window`).
 */
function provider(): InjectedChiaProvider | undefined {
  return typeof window !== "undefined" ? window.chia : undefined;
}

/**
 * True iff the DIG Browser's injected wallet is present. `isDIG` is the
 * explicit, unspoofable marker the native provider sets — we detect on it (not
 * merely the presence of `window.chia`, which a different Chia extension could
 * also define).
 */
export function isInjectedAvailable(): boolean {
  const p = provider();
  return !!(p && p.isDIG);
}

/** True iff the native wallet implements `method`. Static allowlist — the
 *  native wallet returns Sage-shaped responses, so no per-session negotiation. */
export function injectedSessionSupports(method: string): boolean {
  return (INJECTED_METHODS as readonly string[]).includes(method);
}

/**
 * Forward one request to the injected provider. The native provider's
 * request({method, params}) resolves to the wallet's data on success and
 * rejects on user-decline / error — exactly the contract walletConnect.ts's
 * request() has, so call sites need no change. Throws a clear error if the
 * provider is absent (the caller chose the injected backend, so absence here is
 * a real fault, not a normal path).
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export async function injectedRequest<T = any>(
  method: string,
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  params?: any
): Promise<T> {
  const p = provider();
  if (!p || typeof p.request !== "function") {
    throw new Error(
      "DIG Browser wallet is not available — reopen this page in the DIG Browser."
    );
  }
  if (!injectedSessionSupports(method)) {
    throw new Error(`The DIG Browser wallet does not support "${method}".`);
  }
  return p.request({ method, params }) as Promise<T>;
}

/**
 * Connect: ask the native wallet to approve THIS origin. Blocks until the user
 * approves (or rejects) in the native wallet UI. Throws on rejection so the
 * connect flow can surface the decline. A no-op connect() (older provider) is
 * tolerated.
 */
export async function injectedConnect(eager = false): Promise<void> {
  const p = provider();
  if (!p) {
    throw new Error(
      "DIG Browser wallet is not available — reopen this page in the DIG Browser."
    );
  }
  if (typeof p.connect === "function") {
    await p.connect(eager);
  }
}
