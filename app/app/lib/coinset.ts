// ============================================================================
// coinset.ts — JS-side chain access (HTTP fetch to api.coinset.org)
// ============================================================================
//
// MODULE: lib/coinset
// PURPOSE: The chip35-dl-coin-wasm SDK is a hard boundary — it never opens
//          a chain client. The dApp does its own chain reads here, then
//          passes the resolved coins into wasm helpers for assembly + signing.
//
// TRANSPORT: `https://api.coinset.org` is the canonically-synced public
//            Chia full-node REST endpoint. Override with
//            `NEXT_PUBLIC_COINSET_BASE_URL` in `.env.local`.

const COINSET_BASE_URL =
  process.env.NEXT_PUBLIC_COINSET_BASE_URL?.trim() ||
  "https://api.coinset.org";

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

async function postJson<T>(path: string, body: unknown): Promise<T> {
  const r = await fetch(`${COINSET_BASE_URL}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!r.ok) {
    throw new Error(`coinset ${path}: HTTP ${r.status}`);
  }
  return (await r.json()) as T;
}

/** Add `0x` prefix if missing. */
function withPrefix(s: string): string {
  return s.startsWith("0x") ? s : "0x" + s;
}

/** Strip `0x` prefix and lowercase. */
export function stripHex(s: string): string {
  return s.toLowerCase().replace(/^0x/, "");
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/** Chia coin record (camelCase, mojos as number). */
export interface CoinRecord {
  parentCoinInfo: string; // 0x-prefixed hex
  puzzleHash: string; // 0x-prefixed hex
  amount: number;
  spentHeight: number;
  confirmedHeight: number;
}

/** JSON shape that Chia full-node / coinset `push_tx` expects. */
export interface SpendBundleJson {
  coin_spends: {
    coin: { parent_coin_info: string; puzzle_hash: string; amount: number };
    puzzle_reveal: string;
    solution: string;
  }[];
  aggregated_signature: string;
}

// ---------------------------------------------------------------------------
// Raw API types
// ---------------------------------------------------------------------------

interface RawCoin {
  parent_coin_info: string;
  puzzle_hash: string;
  amount: number;
}

interface RawCoinRecord {
  coin: RawCoin;
  spent_block_index: number;
  confirmed_block_index: number;
  spent: boolean;
  coinbase: boolean;
  timestamp: number;
}

function adapt(r: RawCoinRecord): CoinRecord {
  return {
    parentCoinInfo: withPrefix(r.coin.parent_coin_info),
    puzzleHash: withPrefix(r.coin.puzzle_hash),
    amount: Number(r.coin.amount),
    spentHeight: r.spent_block_index,
    confirmedHeight: r.confirmed_block_index,
  };
}

// ---------------------------------------------------------------------------
// Chain reads
// ---------------------------------------------------------------------------

/**
 * Get a single coin record by its coin id (name).
 * Returns null if the coin is not on-chain.
 */
export async function getCoinRecordByName(
  coinIdHex: string
): Promise<CoinRecord | null> {
  try {
    const name = withPrefix(coinIdHex);
    const r = await postJson<{ coin_record: RawCoinRecord | null }>(
      "/get_coin_record_by_name",
      { name }
    );
    return r.coin_record ? adapt(r.coin_record) : null;
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------------
// Confirmation polling
// ---------------------------------------------------------------------------

/**
 * Await-able sleep. No `Date` at module scope; a fresh Promise + setTimeout
 * per call. Works in browser and Node (setTimeout is global in both).
 */
function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export interface WaitOptions {
  /** Overall budget before throwing. Default 180_000 ms (3 min). */
  timeoutMs?: number;
  /** Delay between polls. Default 5_000 ms. */
  intervalMs?: number;
}

/**
 * Poll coinset until a newly-created coin is confirmed on-chain:
 * a coin record exists AND `confirmed_block_index > 0`.
 *
 * Resolves to the confirming CoinRecord. Throws a clear Error on timeout.
 * Used by mint() and updateMetadata() to confirm the new singleton coin.
 */
export async function waitForCoinConfirmation(
  coinIdHex: string,
  { timeoutMs = 180000, intervalMs = 5000 }: WaitOptions = {}
): Promise<CoinRecord> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const rec = await getCoinRecordByName(coinIdHex);
    if (rec && rec.confirmedHeight > 0) {
      return rec;
    }
    if (Date.now() >= deadline) {
      throw new Error(
        `Timed out after ${Math.round(
          timeoutMs / 1000
        )}s waiting for coin ${withPrefix(
          coinIdHex
        )} to be confirmed on-chain.`
      );
    }
    await sleep(intervalMs);
  }
}

/**
 * Poll coinset until the coin with `coinIdHex` is spent:
 * a coin record exists AND `spent_block_index > 0`.
 *
 * Resolves to the spent CoinRecord. Throws a clear Error on timeout.
 * Used by del() to confirm the singleton was melted on-chain.
 */
export async function waitForCoinSpent(
  coinIdHex: string,
  { timeoutMs = 180000, intervalMs = 5000 }: WaitOptions = {}
): Promise<CoinRecord> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const rec = await getCoinRecordByName(coinIdHex);
    if (rec && rec.spentHeight > 0) {
      return rec;
    }
    if (Date.now() >= deadline) {
      throw new Error(
        `Timed out after ${Math.round(
          timeoutMs / 1000
        )}s waiting for coin ${withPrefix(coinIdHex)} to be spent on-chain.`
      );
    }
    await sleep(intervalMs);
  }
}

/** Current blockchain state including peak height. */
export interface BlockchainState {
  peakHeight: number | null;
  difficulty: number | null;
}

/** Get the current blockchain state (peak height, difficulty). */
export async function getBlockchainState(): Promise<BlockchainState> {
  try {
    const r = await postJson<{
      blockchain_state: {
        peak: { height: number; weight?: number } | null;
        difficulty?: number;
      };
    }>("/get_blockchain_state", {});
    return {
      peakHeight: r.blockchain_state?.peak?.height ?? null,
      difficulty: r.blockchain_state?.difficulty ?? null,
    };
  } catch {
    return { peakHeight: null, difficulty: null };
  }
}

// ---------------------------------------------------------------------------
// push_tx
// ---------------------------------------------------------------------------

/**
 * Push a signed spend bundle to the mempool via coinset.org.
 *
 * Returns a status string on success (`"SUCCESS"`, `"ALREADY_INCLUDING_TRANSACTION"`, etc.)
 * Throws on rejection.
 */
export async function pushTx(spendBundle: SpendBundleJson): Promise<string> {
  const raw = await postJson<unknown>("/push_tx", {
    spend_bundle: spendBundle,
  });

  const resp =
    typeof raw === "object" && raw !== null && !Array.isArray(raw)
      ? (raw as Record<string, unknown>)
      : {};

  const status =
    typeof resp.status === "string" ? resp.status : undefined;

  const success =
    resp.success === true ||
    resp.success === 1 ||
    String(resp.success).toLowerCase() === "true" ||
    status === "SUCCESS" ||
    status === "ALREADY_INCLUDING_TRANSACTION";

  if (success) {
    // Return status string on success — no substring scanning needed.
    return status ?? "SUCCESS";
  }

  // Non-success branch: extract a useful error detail.
  const fullSerialized =
    typeof raw === "object" && raw !== null
      ? JSON.stringify(raw)
      : String(raw);
  const fullUpper = fullSerialized.toUpperCase();

  const detail =
    (typeof resp.error === "string" && resp.error) ||
    (typeof resp.detail === "string" && resp.detail) ||
    fullUpper.match(/MINTING_COIN|DOUBLE_SPEND|INVALID_SPEND_BUNDLE/)?.[0] ||
    "unknown error (server returned success=false with no detail)";

  throw new Error(`/push_tx rejected: ${detail}`);
}

export { COINSET_BASE_URL, withPrefix };
