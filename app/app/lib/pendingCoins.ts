// ============================================================================
// pendingCoins.ts — localStorage-backed set of recently-spent coin ids
// ============================================================================
//
// PURPOSE: Prevent DOUBLE_SPEND errors by remembering which XCH coins have
//          already been submitted to the mempool, so that an immediate retry
//          does not reuse the same funding coin.
//
// BROWSER-ONLY: all functions guard localStorage access with
// `typeof window !== "undefined"`. Safe to import server-side.
//
// DESIGN: Callers pass `nowMs` so there is no `Date` call at module scope
//         and tests can control time injection.

const STORAGE_KEY = "chip35_pending_coins";
const DEFAULT_TTL_MS = 900_000; // 15 minutes

interface PendingEntry {
  spentAtMs: number;
}

// ---------------------------------------------------------------------------
// Internal helpers — guarded reads/writes
// ---------------------------------------------------------------------------

function readMap(): Record<string, PendingEntry> {
  if (typeof window === "undefined") return {};
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    return JSON.parse(raw) as Record<string, PendingEntry>;
  } catch {
    return {};
  }
}

function writeMap(map: Record<string, PendingEntry>): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(map));
  } catch (e) {
    console.warn("[chip35/pendingCoins] Failed to persist to localStorage:", e);
  }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Mark a coin id as recently spent so future coin-selection skips it.
 * @param coinIdHex  Lowercase hex coin id (with or without 0x prefix).
 * @param nowMs      Current timestamp in milliseconds (pass `Date.now()`).
 */
export function markCoinSpent(coinIdHex: string, nowMs: number): void {
  const key = coinIdHex.toLowerCase().replace(/^0x/, "");
  const map = readMap();
  map[key] = { spentAtMs: nowMs };
  writeMap(map);
}

/**
 * Return true if the coin id was marked spent within the TTL window.
 * Entries older than `ttlMs` are ignored (treated as stale and pruned).
 *
 * @param coinIdHex  Lowercase hex coin id (with or without 0x prefix).
 * @param nowMs      Current timestamp in milliseconds (pass `Date.now()`).
 * @param ttlMs      How long to consider an entry as pending (default 15 min).
 */
export function isCoinPendingSpent(
  coinIdHex: string,
  nowMs: number,
  ttlMs: number = DEFAULT_TTL_MS
): boolean {
  const key = coinIdHex.toLowerCase().replace(/^0x/, "");
  const map = readMap();
  const entry = map[key];
  if (!entry) return false;
  const age = nowMs - entry.spentAtMs;
  if (age > ttlMs) {
    // Stale — prune it
    delete map[key];
    writeMap(map);
    return false;
  }
  return true;
}

/**
 * Explicitly remove a coin id from the pending-spent set (e.g. after
 * on-chain confirmation when we know it can safely be freed).
 *
 * @param coinIdHex  Lowercase hex coin id (with or without 0x prefix).
 */
export function clearCoinSpent(coinIdHex: string): void {
  const key = coinIdHex.toLowerCase().replace(/^0x/, "");
  const map = readMap();
  if (key in map) {
    delete map[key];
    writeMap(map);
  }
}
