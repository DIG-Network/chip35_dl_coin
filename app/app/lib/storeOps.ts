// ============================================================================
// storeOps.ts — orchestration: mint / updateMetadata / delete DataLayer stores
// ============================================================================
//
// MODULE: lib/storeOps
// PURPOSE: High-level async operations that the UI dispatches.
//          Each function wires together:
//            getWasm() → WalletConnect → coinset push
//          and persists the result to the registry.
//
// BROWSER-ONLY: never call these at module scope or during SSR.

import { getWasm } from "./wasm";
import {
  getAddress,
  getAssetCoins,
  signCoinSpends,
} from "./walletConnect";
import {
  puzzleHashBytesFromAddress,
  syntheticPkHexFromCoinPuzzle,
} from "./chiaAddress";
import { pushTx } from "./coinset";
import {
  coinSpendToWallet,
  toSpendBundleJson,
  dataStoreToRegistryJson,
  dataStoreFromRegistryJson,
  hex0xToBytes,
  bytesToHex,
  coinId,
  type DataStoreWasm,
} from "./convert";
import {
  upsertStore,
  markDeleted,
  getStore,
  type RegistryEntry,
} from "./registry";

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/**
 * Validate that `sig` is a 96-byte aggregated BLS signature (192 hex chars).
 * Throws a descriptive error if the shape is wrong, returns the original sig
 * string if it is valid (with or without leading `0x` — callers normalise it).
 */
function assertAggSigHex(sig: string): string {
  const h = sig.startsWith("0x") ? sig.slice(2) : sig;
  if (!/^[0-9a-fA-F]{192}$/.test(h)) {
    throw new Error(
      `Wallet returned an invalid aggregated signature (expected 96 bytes / 192 hex chars, got ${h.length} hex chars)`
    );
  }
  return sig;
}

/** Pick an XCH coin from Sage that covers `minMojos`. Returns null if none found. */
async function pickXchCoin(minMojos: bigint): Promise<{
  coin: { parentCoinInfo: Uint8Array; puzzleHash: Uint8Array; amount: bigint };
  syntheticPkHex: string;
} | null> {
  const raw = await getAssetCoins(null, null, false, 0, 200);
  if (!raw) return null;

  for (const entry of raw) {
    if (!entry?.coin || !entry.puzzle) continue;
    const amount = BigInt(entry.coin.amount);
    if (amount < minMojos) continue;

    const pk = await syntheticPkHexFromCoinPuzzle(entry.puzzle);
    if (!pk) continue;

    // Convert snake_case coin to wasm shape
    const parentCoinInfo = hex0xToBytes(entry.coin.parent_coin_info);
    const puzzleHash = hex0xToBytes(entry.coin.puzzle_hash);

    return {
      coin: { parentCoinInfo, puzzleHash, amount },
      syntheticPkHex: pk,
    };
  }
  return null;
}

// ---------------------------------------------------------------------------
// mint
// ---------------------------------------------------------------------------

export interface MintParams {
  label?: string;
  description?: string;
  /** Root hash hex (32 bytes, with or without 0x). */
  rootHashHex: string;
  /** Fee in mojos (bigint). */
  feeMojos: bigint;
}

export interface MintResult {
  launcherIdHex: string;
  currentCoinIdHex: string;
}

/**
 * Mint a new CHIP-0035 DataLayer store.
 *
 * Flow:
 *   1. Get wallet address → derive owner puzzle hash.
 *   2. Get XCH coins from Sage; pick one covering fee + 1 mojo (singleton amount).
 *   3. Uncurry its puzzle reveal → synthetic_pk (minter key).
 *   4. Call wasm `mintStore(...)`.
 *   5. Convert coin spends → WalletConnect wire format.
 *   6. Sign via Sage → push to coinset.
 *   7. Compute the new store's current coin id and persist registry entry.
 */
export async function mint(params: MintParams): Promise<MintResult> {
  // 1. Wallet address + puzzle hash
  const address = getAddress();
  if (!address) throw new Error("Wallet not connected.");

  const ownerPuzzleHash = await puzzleHashBytesFromAddress(address);
  if (!ownerPuzzleHash) throw new Error(`Invalid wallet address: ${address}`);

  // 2 + 3. Pick a coin; need at least fee + 1 mojo for the singleton
  const minMojos = params.feeMojos + 1n;
  const selected = await pickXchCoin(minMojos);
  if (!selected) {
    throw new Error(
      `No spendable XCH coin found covering ${minMojos} mojos (fee + 1 for singleton). ` +
        "Please fund the wallet and try again."
    );
  }

  // 4. Call wasm mintStore
  const wasm = await getWasm();
  const rootHash = hex0xToBytes(params.rootHashHex);
  const minterSyntheticKey = hex0xToBytes(selected.syntheticPkHex);

  // wasm expects selected_coins as an array-like JS value
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const result = wasm.mintStore(
    minterSyntheticKey,
    [selected.coin] as unknown as object,
    rootHash,
    params.label ?? null,
    params.description ?? null,
    undefined, // bytes
    undefined, // sizeProof
    ownerPuzzleHash,
    [] as unknown as object, // delegatedPuzzles — empty for basic mint
    params.feeMojos
  ) as { coinSpends: unknown[]; newStore: unknown };

  const coinSpends = result.coinSpends as Array<{
    coin: { parentCoinInfo: Uint8Array; puzzleHash: Uint8Array; amount: bigint };
    puzzleReveal: Uint8Array;
    solution: Uint8Array;
  }>;
  const newStore = result.newStore as DataStoreWasm;

  // 5. Convert to WalletConnect wire format
  const wcCoinSpends = coinSpends.map(coinSpendToWallet);

  // 6. Sign via Sage
  const aggSig = assertAggSigHex(await signCoinSpends(wcCoinSpends, false, false) ?? "");
  if (!aggSig) throw new Error("Sage rejected signing or returned no signature.");

  // Push to coinset
  const spendBundle = toSpendBundleJson(coinSpends, aggSig);
  await pushTx(spendBundle);

  // 7. Compute coin id for the new singleton's coin
  const currentCoinIdHex = await coinId(newStore.coin);
  const launcherIdHex = "0x" + bytesToHex(newStore.launcherId);

  // Persist to registry
  const now = Date.now();
  const entry: RegistryEntry = {
    launcherId: launcherIdHex,
    label: params.label ?? "",
    dataStoreJson: dataStoreToRegistryJson(newStore),
    ownerSyntheticPkHex: selected.syntheticPkHex,
    currentCoinIdHex,
    status: "live",
    history: [{ ts: now, op: "mint" }],
  };
  upsertStore(entry);

  return { launcherIdHex, currentCoinIdHex };
}

// ---------------------------------------------------------------------------
// updateMetadata
// ---------------------------------------------------------------------------

export interface UpdateMetadataParams {
  newRootHashHex: string;
  newLabel?: string;
  newDescription?: string;
  newBytes?: bigint;
}

/**
 * Update the metadata of an existing DataLayer store.
 *
 * Uses the owner synthetic key stored in the registry for signing.
 * The registry entry is updated with the returned `newStore`.
 */
export async function updateMetadata(
  launcherId: string,
  params: UpdateMetadataParams
): Promise<void> {
  const entry = getStore(launcherId);
  if (!entry) throw new Error(`Store not found in registry: ${launcherId}`);
  if (entry.status === "deleted")
    throw new Error(`Store ${launcherId} has been melted.`);

  const store = dataStoreFromRegistryJson(entry.dataStoreJson);
  const ownerPublicKey = hex0xToBytes(entry.ownerSyntheticPkHex);
  const newRootHash = hex0xToBytes(params.newRootHashHex);

  const wasm = await getWasm();
  const result = wasm.updateStoreMetadata(
    store as unknown as object,
    newRootHash,
    params.newLabel ?? null,
    params.newDescription ?? null,
    params.newBytes ?? null,
    null, // newSizeProof
    ownerPublicKey,
    null, // adminPublicKey
    null  // writerPublicKey
  ) as { coinSpends: unknown[]; newStore: unknown };

  const coinSpends = result.coinSpends as Array<{
    coin: { parentCoinInfo: Uint8Array; puzzleHash: Uint8Array; amount: bigint };
    puzzleReveal: Uint8Array;
    solution: Uint8Array;
  }>;
  const newStore = result.newStore as DataStoreWasm;

  const wcCoinSpends = coinSpends.map(coinSpendToWallet);
  const aggSig = assertAggSigHex(await signCoinSpends(wcCoinSpends, false, false) ?? "");
  if (!aggSig) throw new Error("Sage rejected signing or returned no signature.");

  const spendBundle = toSpendBundleJson(coinSpends, aggSig);
  await pushTx(spendBundle);

  const currentCoinIdHex = await coinId(newStore.coin);
  const now = Date.now();

  const updatedEntry: RegistryEntry = {
    ...entry,
    label: params.newLabel ?? entry.label,
    dataStoreJson: dataStoreToRegistryJson(newStore),
    currentCoinIdHex,
    history: [
      ...entry.history,
      { ts: now, op: "updateMetadata" },
    ],
  };
  upsertStore(updatedEntry);
}

// ---------------------------------------------------------------------------
// del (melt)
// ---------------------------------------------------------------------------

/**
 * Melt (permanently delete) a DataLayer store.
 *
 * The singleton is spent with `meltStore`; on success the registry
 * entry is marked "deleted".
 */
export async function del(launcherId: string): Promise<void> {
  const entry = getStore(launcherId);
  if (!entry) throw new Error(`Store not found in registry: ${launcherId}`);
  if (entry.status === "deleted")
    throw new Error(`Store ${launcherId} is already deleted.`);

  const store = dataStoreFromRegistryJson(entry.dataStoreJson);
  const ownerPublicKey = hex0xToBytes(entry.ownerSyntheticPkHex);

  const wasm = await getWasm();
  const coinSpends = wasm.meltStore(
    store as unknown as object,
    ownerPublicKey
  ) as Array<{
    coin: { parentCoinInfo: Uint8Array; puzzleHash: Uint8Array; amount: bigint };
    puzzleReveal: Uint8Array;
    solution: Uint8Array;
  }>;

  const wcCoinSpends = coinSpends.map(coinSpendToWallet);
  const aggSig = assertAggSigHex(await signCoinSpends(wcCoinSpends, false, false) ?? "");
  if (!aggSig) throw new Error("Sage rejected signing or returned no signature.");

  const spendBundle = toSpendBundleJson(coinSpends, aggSig);
  await pushTx(spendBundle);

  markDeleted(launcherId, Date.now());
}
