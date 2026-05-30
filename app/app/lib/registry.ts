// ============================================================================
// registry.ts — localStorage CRUD for chip35 DataLayer store entries
// ============================================================================
//
// MODULE: lib/registry
// PURPOSE: Persist discovered/minted store records locally so the UI
//          can list stores without a chain-scan.
//
// STORAGE KEY: "chip35_stores" → JSON array of RegistryEntry[]
//
// BROWSER-ONLY: all functions guard localStorage access with
// `typeof window !== "undefined"`. Safe to import server-side.

import type { DataStoreJson } from "./convert";

const STORAGE_KEY = "chip35_stores";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface RegistryHistoryEntry {
  /** Caller-supplied timestamp (ms since epoch). */
  ts: number;
  op: string;
}

export interface RegistryEntry {
  /** Launcher id — 0x-prefixed 64-char hex. Used as the unique key. */
  launcherId: string;
  /** Human-readable label (may be empty string). */
  label: string;
  /** Full DataStore serialised to JSON-safe form. */
  dataStoreJson: DataStoreJson;
  /** Owner synthetic pubkey (0x-prefixed 96-char hex). */
  ownerSyntheticPkHex: string;
  /**
   * Current singleton coin id (lowercase hex, NO 0x).
   * Updated after each successful spend. Used for liveness checks.
   */
  currentCoinIdHex: string;
  /**
   * Lifecycle status of the store coin:
   *   "pending"   — spend pushed; awaiting on-chain confirmation.
   *   "confirmed" — the current coin is confirmed on-chain.
   *   "deleted"   — meltStore confirmed (coin spent, singleton retired).
   * Legacy entries may carry "live" — treat that as "confirmed" for display
   * (see `displayStatus`).
   */
  status: RegistryStatus;
  /** Operation history. */
  history: RegistryHistoryEntry[];
}

/** Current status values. `"live"` is a legacy alias kept for old entries. */
export type RegistryStatus = "pending" | "confirmed" | "deleted" | "live";

/**
 * Normalise a stored status for display. Legacy `"live"` entries (written
 * before confirmation tracking existed) are surfaced as `"confirmed"`.
 */
export function displayStatus(
  status: RegistryStatus
): "pending" | "confirmed" | "deleted" {
  if (status === "live") return "confirmed";
  return status;
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

function readAll(): RegistryEntry[] {
  if (typeof window === "undefined") return [];
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    return JSON.parse(raw) as RegistryEntry[];
  } catch {
    return [];
  }
}

function writeAll(entries: RegistryEntry[]): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(entries));
  } catch (e) {
    console.error("[chip35/registry] Failed to persist to localStorage:", e);
  }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/** List all registry entries (both live and deleted). */
export function listStores(): RegistryEntry[] {
  return readAll();
}

/** Get a single entry by launcher id (with or without 0x prefix). Returns undefined if not found. */
export function getStore(launcherId: string): RegistryEntry | undefined {
  const key = launcherId.toLowerCase().replace(/^0x/, "");
  return readAll().find(
    (e) => e.launcherId.toLowerCase().replace(/^0x/, "") === key
  );
}

/**
 * Insert or update a registry entry. Matched by `entry.launcherId`.
 * Caller is responsible for setting `entry.history` entries with correct timestamps.
 */
export function upsertStore(entry: RegistryEntry): void {
  const all = readAll();
  const key = entry.launcherId.toLowerCase().replace(/^0x/, "");
  const idx = all.findIndex(
    (e) => e.launcherId.toLowerCase().replace(/^0x/, "") === key
  );
  if (idx >= 0) {
    all[idx] = entry;
  } else {
    all.push(entry);
  }
  writeAll(all);
}

/**
 * Mark a store as deleted (does not remove it from the list so history is preserved).
 * @param launcherId  With or without 0x prefix.
 * @param ts          Caller-supplied timestamp (ms since epoch).
 */
export function markDeleted(launcherId: string, ts: number): void {
  const all = readAll();
  const key = launcherId.toLowerCase().replace(/^0x/, "");
  const entry = all.find(
    (e) => e.launcherId.toLowerCase().replace(/^0x/, "") === key
  );
  if (!entry) return;
  entry.status = "deleted";
  entry.history.push({ ts, op: "melt" });
  writeAll(all);
}
