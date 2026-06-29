// ============================================================================
// walletConnect.ts — Sage Wallet RPC bridge via WalletConnect
// ============================================================================
//
// MODULE: lib/walletConnect
// PURPOSE: Singleton WalletConnect (`@walletconnect/sign-client`) wrapper
//          that talks to Sage Wallet (Chia-aware light wallet exposing
//          CHIP-0002 / native chia_* methods over WalletConnect).
//
// SUPPORTED RPCs:
//   * chia_getAddress              — current XCH address (bech32m)
//   * chip0002_getPublicKeys       — synthetic pubkeys for the active key
//   * chip0002_getAssetCoins       — spendable coins with puzzle reveals
//   * chip0002_signCoinSpends      — sign an unsigned coin-spend list
//
// BROWSER-ONLY: SignClient opens IndexedDB at init time; this module
// guards all paths with `typeof window` checks to be safe during Next.js's
// server prerender pass.
//
// TRANSPORT SWAP — DIG Browser injected wallet (window.chia):
// When the page is opened inside the DIG Browser, `window.chia.isDIG` is
// present. We then PREFER the in-process wallet over WalletConnect: no QR, no
// relay, no pairing. The native provider returns the SAME Sage-shaped responses
// (chia_getAddress / chip0002_getAssetCoins / chip0002_signCoinSpends), so the
// callers in storeOps.ts and the parsing here are unchanged — only the
// TRANSPORT is swapped. When `window.chia` is absent the WalletConnect path
// below runs EXACTLY as before (zero regression). This mirrors hub.dig.net's
// injected-wallet.js + wallet-transport.js pattern.

import SignClient from "@walletconnect/sign-client";
import { SessionTypes } from "@walletconnect/types";
import {
  isInjectedAvailable,
  injectedConnect,
  injectedRequest,
} from "./injectedWallet";

// ---------------------------------------------------------------------------
// Wire types — JSON shapes matching Sage's RPC convention
// ---------------------------------------------------------------------------

/** JSON-shaped Coin matching the WalletConnect/Sage RPC convention. */
export interface WcCoin {
  parent_coin_info: string;
  puzzle_hash: string;
  amount: number;
}

/** JSON-shaped CoinSpend matching the WalletConnect/Sage RPC convention. */
export interface WcCoinSpend {
  coin: WcCoin;
  puzzle_reveal: string;
  solution: string;
}

/**
 * Per-coin entry returned by chip0002_getAssetCoins. The `puzzle`
 * field carries the CLVM-serialized puzzle reveal. Uncurry it via
 * chia-wallet-sdk-wasm `Clvm().deserialize(bytes).uncurry()` to get
 * the synthetic_pk (first curried arg of the standard p2 puzzle).
 */
export interface SageAssetCoin {
  coin: WcCoin;
  coinName?: string;
  /** CLVM-serialized puzzle reveal — hex-encoded. */
  puzzle?: string;
  confirmedBlockIndex?: number;
  spentBlockIndex?: number;
  locked?: boolean;
  lineageProof?: {
    parentCoinInfo?: string;
    parent_coin_info?: string;
    innerPuzzleHash?: string;
    inner_puzzle_hash?: string;
    amount?: number;
  };
}

// ---------------------------------------------------------------------------
// Singleton state
// ---------------------------------------------------------------------------

let _client: SignClient | undefined;
let _session: SessionTypes.Struct | undefined;
let _address: string | undefined;
let _initPromise: Promise<void> | undefined;

// Event listeners set up after first client init
let _listenersAttached = false;

// True once we've connected via the DIG Browser's injected provider. While set,
// every RPC below routes through window.chia instead of the WalletConnect relay.
// (There is no per-session topic for the injected provider — it's one
// in-process wallet keyed on the page origin — so this boolean is the whole
// session state for that backend.)
let _injectedActive = false;

/**
 * Sentinel pairing URI returned by connect() when the injected backend is used.
 * There is no QR/relay URI in that case; the UI checks for this value to skip
 * the QR modal and go straight to the connected state.
 */
export const INJECTED_URI = "injected:dig-browser";

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

function getProjectId(): string | undefined {
  return process.env.NEXT_PUBLIC_WALLETCONNECT_PROJECT_ID;
}

async function _initClient(): Promise<SignClient | undefined> {
  if (_client) return _client;
  if (typeof window === "undefined") return undefined;

  const projectId = getProjectId();
  if (!projectId) {
    console.error(
      "[chip35/WalletConnect] Missing NEXT_PUBLIC_WALLETCONNECT_PROJECT_ID. " +
        "Create one at https://cloud.reown.com and add it to app/.env.local."
    );
    return undefined;
  }

  const origin =
    typeof window !== "undefined" && window.location
      ? window.location.origin
      : "https://dig.net";

  _client = await SignClient.init({
    logger: "error",
    projectId,
    metadata: {
      name: "DIG Network — CHIP-0035 Store Demo",
      description:
        "A DIG Network demo: mint / advance (new capsule) / melt CHIP-0035 stores on Chia via Sage Wallet",
      url: origin,
      icons: ["https://avatars.githubusercontent.com/u/37784886"],
    },
  });

  if (!_listenersAttached) {
    _client.on("session_delete", () => _handleDisconnect());
    _client.on("session_expire", () => _handleDisconnect());
    _listenersAttached = true;
  }

  return _client;
}

function _handleDisconnect() {
  _session = undefined;
  _address = undefined;
  _injectedActive = false;
}

/**
 * If the DIG Browser already approved this origin, restore the address silently
 * (eager connect) so a reload comes back connected without a fresh prompt. Best
 * effort: any failure leaves us disconnected and the user can connect manually.
 */
async function _restoreInjectedSession(): Promise<boolean> {
  if (!isInjectedAvailable()) return false;
  try {
    await injectedConnect(true); // eager: don't force an approval prompt
    const resp = await injectedRequest<{ address: string }>(
      "chia_getAddress",
      {}
    );
    if (resp?.address) {
      _injectedActive = true;
      _address = resp.address;
      return true;
    }
  } catch {
    // Not yet approved for this origin (or older provider) — fall through.
  }
  return false;
}

async function _restoreSession(): Promise<void> {
  if (!_client) return;
  try {
    const sessions = _client.session.getAll();
    for (const s of sessions) {
      if (_client.session.keys.includes(s.topic)) {
        try {
          const resp = await _client.request<{ address: string }>({
            topic: s.topic,
            chainId: "chia:mainnet",
            request: { method: "chia_getAddress", params: {} },
          });
          if (resp?.address) {
            _session = s;
            _address = resp.address;
            return;
          }
        } catch {
          // stale session; ignore
        }
      }
    }
  } catch (e) {
    console.warn("[chip35/WalletConnect] session restore failed:", e);
  }
}

// ---------------------------------------------------------------------------
// Exported singleton API
// ---------------------------------------------------------------------------

/**
 * Ensure the SignClient is initialised and any previous session restored.
 * Safe to call multiple times — runs only once. Browser-only.
 */
export async function init(): Promise<void> {
  if (typeof window === "undefined") return;
  if (_initPromise) return _initPromise;
  _initPromise = (async () => {
    // Inside the DIG Browser, PREFER the injected wallet: try a silent restore
    // and skip WalletConnect init entirely (no relay/IndexedDB, and no need for
    // a WalletConnect project id). Outside it, run the WC flow exactly as before.
    if (isInjectedAvailable()) {
      await _restoreInjectedSession();
      return;
    }
    await _initClient();
    await _restoreSession();
  })();
  return _initPromise;
}

/**
 * Initiate a wallet connection.
 *
 * Returns `{ uri, approvalPromise }` so the caller can:
 *   1. Display `uri` as a QR code (using `qrcode.react`).
 *   2. `await approvalPromise` to get the address once the user approves.
 *
 * DIG Browser: when `window.chia.isDIG` is present, `uri` is the `INJECTED_URI`
 * sentinel (no QR) and `approvalPromise` resolves once the user approves this
 * origin in the native wallet UI. Otherwise the WalletConnect pairing runs
 * exactly as before.
 *
 * Throws if the WalletConnect client cannot be initialised (missing project id,
 * etc.) on the non-injected path.
 */
export async function connect(): Promise<{
  uri: string;
  approvalPromise: Promise<string | undefined>;
}> {
  // DIG Browser injected path: approve this origin in the native wallet, then
  // read the address. No QR/relay — return the sentinel uri immediately.
  if (isInjectedAvailable()) {
    const approvalPromise = (async (): Promise<string | undefined> => {
      try {
        await injectedConnect(false); // blocks on the native approval UI
        const resp = await injectedRequest<{ address: string }>(
          "chia_getAddress",
          {}
        );
        _injectedActive = true;
        _address = resp?.address;
        return _address;
      } catch (e) {
        console.error("[chip35/InjectedWallet] connect failed:", e);
        return undefined;
      }
    })();
    return { uri: INJECTED_URI, approvalPromise };
  }

  const client = await _initClient();
  if (!client) {
    throw new Error(
      "WalletConnect client could not be initialised. " +
        "Check NEXT_PUBLIC_WALLETCONNECT_PROJECT_ID."
    );
  }

  const { uri, approval } = await client.connect({
    optionalNamespaces: {
      chia: {
        chains: ["chia:mainnet"],
        methods: [
          "chia_getAddress",
          "chip0002_getAssetCoins",
          "chip0002_signCoinSpends",
        ],
        events: [],
      },
    },
  });

  if (!uri) {
    throw new Error("WalletConnect did not return a pairing URI.");
  }

  const approvalPromise = (async (): Promise<string | undefined> => {
    try {
      const session = await approval();
      _session = session;
      const resp = await client.request<{ address: string }>({
        topic: session.topic,
        chainId: "chia:mainnet",
        request: { method: "chia_getAddress", params: {} },
      });
      _address = resp?.address;
      return _address;
    } catch (e) {
      console.error("[chip35/WalletConnect] approval failed:", e);
      return undefined;
    }
  })();

  return { uri, approvalPromise };
}

/** Return the connected wallet address (bech32m), or undefined if not connected. */
export function getAddress(): string | undefined {
  return _address;
}

/** Return the active WalletConnect session, or undefined. */
export function getSession(): SessionTypes.Struct | undefined {
  return _session;
}

/**
 * Fetch spendable coins from Sage.
 *
 * @param type  `null` for XCH; `'cat' | 'nft' | 'did'` for asset classes.
 * @param assetId  Asset id (TAIL hash hex, no 0x) for CATs; null for XCH.
 * @param includedLocked  Include locked/clawback coins.
 * @param offset  Pagination offset.
 * @param limit   Max coins to return.
 */
export async function getAssetCoins(
  type: null | "cat" | "nft" | "did",
  assetId: string | null,
  includedLocked: boolean,
  offset: number,
  limit: number
): Promise<SageAssetCoin[] | undefined> {
  const params = { type, assetId, includedLocked, offset, limit };
  if (_injectedActive) {
    try {
      return await injectedRequest<SageAssetCoin[]>(
        "chip0002_getAssetCoins",
        params
      );
    } catch (e) {
      console.error("[chip35/InjectedWallet] getAssetCoins failed:", e);
      return undefined;
    }
  }
  if (!_client || !_session) return undefined;
  try {
    const response = await _client.request<SageAssetCoin[]>({
      topic: _session.topic,
      chainId: "chia:mainnet",
      request: {
        method: "chip0002_getAssetCoins",
        params,
      },
    });
    return response;
  } catch (e) {
    console.error("[chip35/WalletConnect] getAssetCoins failed:", e);
    return undefined;
  }
}

/**
 * Sign (and optionally auto-submit) a list of coin spends.
 *
 * @param coinSpends  Array of `WcCoinSpend` objects (snake_case, 0x-hex).
 * @param partial     True for partial signing (multi-sig).
 * @param autoSubmit  When true Sage broadcasts the bundle itself.
 * @returns  The aggregated BLS signature hex (96-byte, possibly 0x-prefixed),
 *           or undefined on failure.
 */
export async function signCoinSpends(
  coinSpends: WcCoinSpend[],
  partial: boolean,
  autoSubmit: boolean
): Promise<string | undefined> {
  const params = { coinSpends, partial, auto_submit: autoSubmit };
  if (_injectedActive) {
    try {
      return await injectedRequest<string>("chip0002_signCoinSpends", params);
    } catch (e) {
      console.error("[chip35/InjectedWallet] signCoinSpends failed:", e);
      return undefined;
    }
  }
  if (!_client || !_session) return undefined;
  try {
    const response = await _client.request<string>({
      topic: _session.topic,
      chainId: "chia:mainnet",
      request: {
        method: "chip0002_signCoinSpends",
        params,
      },
    });
    return response;
  } catch (e) {
    console.error("[chip35/WalletConnect] signCoinSpends failed:", e);
    return undefined;
  }
}

/**
 * Disconnect and clean up the active session.
 */
export async function disconnect(): Promise<void> {
  // Injected backend: there is no relay session to tear down — the in-process
  // wallet keeps its own per-origin consent. Just drop our local state so the
  // UI returns to the disconnected view.
  if (!_injectedActive && _client && _session?.topic) {
    try {
      await _client.disconnect({
        topic: _session.topic,
        reason: { code: 6000, message: "User disconnected." },
      });
    } catch (e) {
      console.warn("[chip35/WalletConnect] disconnect error:", e);
    }
  }
  _handleDisconnect();
}

/** True when a wallet session is active (injected provider or WalletConnect). */
export function isConnected(): boolean {
  if (_injectedActive) return !!_address;
  return !!_address && !!_session;
}
