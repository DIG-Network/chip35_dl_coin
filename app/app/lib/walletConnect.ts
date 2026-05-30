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

import SignClient from "@walletconnect/sign-client";
import { SessionTypes } from "@walletconnect/types";

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
      name: "CHIP-0035 DataLayer Store Demo",
      description:
        "List / mint / update / delete CHIP-0035 DataLayer stores on Chia via Sage Wallet",
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
    await _initClient();
    await _restoreSession();
  })();
  return _initPromise;
}

/**
 * Initiate a new WalletConnect pairing.
 *
 * Returns `{ uri, approvalPromise }` so the caller can:
 *   1. Display `uri` as a QR code (using `qrcode.react`).
 *   2. `await approvalPromise` to get the address once the user approves.
 *
 * Throws if the client cannot be initialised (missing project id, etc.).
 */
export async function connect(): Promise<{
  uri: string;
  approvalPromise: Promise<string | undefined>;
}> {
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
  if (!_client || !_session) return undefined;
  try {
    const response = await _client.request<SageAssetCoin[]>({
      topic: _session.topic,
      chainId: "chia:mainnet",
      request: {
        method: "chip0002_getAssetCoins",
        params: { type, assetId, includedLocked, offset, limit },
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
  if (!_client || !_session) return undefined;
  try {
    const response = await _client.request<string>({
      topic: _session.topic,
      chainId: "chia:mainnet",
      request: {
        method: "chip0002_signCoinSpends",
        params: { coinSpends, partial, auto_submit: autoSubmit },
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
  if (_client && _session?.topic) {
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

/** True when a wallet session is active. */
export function isConnected(): boolean {
  return !!_address && !!_session;
}
