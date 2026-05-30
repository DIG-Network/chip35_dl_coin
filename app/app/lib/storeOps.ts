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
  standardPuzzleHashHexFromSyntheticPkHex,
  syntheticPkHexFromCoinPuzzle,
} from "./chiaAddress";
import {
  pushTx,
  waitForCoinConfirmation,
  waitForCoinSpent,
  getCoinRecordByName,
} from "./coinset";
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
import {
  markCoinSpent,
  isCoinPendingSpent,
} from "./pendingCoins";

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

// Wasm coin shape (matches addFee / mintStore parameter types)
type WasmCoin = {
  parentCoinInfo: Uint8Array;
  puzzleHash: Uint8Array;
  amount: bigint;
};

// Wasm CoinSpend shape
type WasmCoinSpend = {
  coin: WasmCoin;
  puzzleReveal: Uint8Array;
  solution: Uint8Array;
};

/**
 * Result returned by pickXchCoin — includes the coin id hex so callers
 * can mark it as spent after a successful push.
 */
interface PickedCoin {
  coin: WasmCoin;
  syntheticPkHex: string;
  /** Lowercase hex coin id (no 0x prefix). */
  coinIdHex: string;
}

/**
 * Pick an XCH coin from Sage that covers `minMojos`.
 *
 * Selection algorithm (robust, avoids DOUBLE_SPEND):
 *   1. Fetch up to 200 XCH coins from Sage.
 *   2. Filter out locked coins, already-spent coins, and coins with no puzzle.
 *   3. Keep only coins with amount >= minMojos. Sort ascending (smallest first).
 *   4. For each candidate:
 *      a. Compute its coin id.
 *      b. Skip if marked as locally pending-spent (localStorage TTL 15 min).
 *      c. Verify on-chain via getCoinRecordByName: if record exists and spent
 *         index > 0, skip (coin already spent on-chain).
 *      d. Accept the first candidate that passes all checks.
 *   5. Return null if no qualifying coin found.
 */
async function pickXchCoin(minMojos: bigint): Promise<PickedCoin | null> {
  const raw = await getAssetCoins(null, null, false, 0, 200);
  if (!raw) return null;

  // Build candidate list: filter, then sort ascending by amount
  const candidates = raw.filter((entry) => {
    if (!entry?.coin || !entry.puzzle) return false;
    if (entry.locked) return false;
    // Both camelCase and snake_case variants may appear depending on Sage version
    const spentIdx =
      (entry as { spent_block_index?: number }).spent_block_index ??
      entry.spentBlockIndex ??
      0;
    if (spentIdx) return false;
    const amount = BigInt(entry.coin.amount);
    if (amount < minMojos) return false;
    return true;
  });

  candidates.sort((a, b) => {
    const diff = BigInt(a.coin.amount) - BigInt(b.coin.amount);
    return diff < 0n ? -1 : diff > 0n ? 1 : 0;
  });

  const nowMs = Date.now();

  for (const entry of candidates) {
    const pk = await syntheticPkHexFromCoinPuzzle(entry.puzzle!);
    if (!pk) continue;

    const parentCoinInfo = hex0xToBytes(entry.coin.parent_coin_info);
    const puzzleHash = hex0xToBytes(entry.coin.puzzle_hash);
    const amount = BigInt(entry.coin.amount);
    const wasmCoin: WasmCoin = { parentCoinInfo, puzzleHash, amount };

    // Compute coin id
    const idHex = await coinId(wasmCoin);

    // Local pending-spent check
    if (isCoinPendingSpent(idHex, nowMs)) continue;

    // On-chain verification: skip if the fullnode says it's spent
    const record = await getCoinRecordByName(idHex);
    if (record && (record.spentHeight ?? 0) > 0) {
      // Coin is already spent on-chain; mark it locally so we don't re-check
      markCoinSpent(idHex, nowMs);
      continue;
    }

    return { coin: wasmCoin, syntheticPkHex: pk, coinIdHex: idHex };
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
export async function mint(
  params: MintParams,
  onStatus?: (s: string) => void
): Promise<MintResult> {
  // 1. Require a connected wallet.
  const address = getAddress();
  if (!address) throw new Error("Wallet not connected.");

  // 2 + 3. Pick a coin; need at least fee + 1 mojo for the singleton.
  const minMojos = params.feeMojos + 1n;
  const selected = await pickXchCoin(minMojos);
  if (!selected) {
    throw new Error(
      `No spendable XCH coin found covering ${minMojos} mojos (fee + 1 for singleton). ` +
        "Please fund the wallet and try again."
    );
  }

  // The store owner MUST be the SAME key that controls the funding coin —
  // the key Sage will sign future update/melt owner-spends with. Using the
  // wallet's "current address" key (frequently a different derivation index
  // than the funding coin) makes the on-chain singleton commit to one owner
  // puzzle hash while melt/update reconstruct another → WRONG_PUZZLE_HASH on
  // push. Deriving the owner from the funding coin's synthetic key keeps the
  // store self-consistent across its whole lifecycle.
  const ownerPuzzleHash = hex0xToBytes(
    await standardPuzzleHashHexFromSyntheticPkHex(selected.syntheticPkHex)
  );

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

  const coinSpends = result.coinSpends as WasmCoinSpend[];
  const newStore = result.newStore as DataStoreWasm;

  // 5. Convert to WalletConnect wire format
  const wcCoinSpends = coinSpends.map(coinSpendToWallet);

  // 6. Sign via Sage
  const aggSig = assertAggSigHex(await signCoinSpends(wcCoinSpends, false, false) ?? "");
  if (!aggSig) throw new Error("Sage rejected signing or returned no signature.");

  // Push to coinset
  onStatus?.("Pushing spend bundle…");
  const spendBundle = toSpendBundleJson(coinSpends, aggSig);
  try {
    await pushTx(spendBundle);
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    // Mark the coin as spent regardless — it may be in the mempool from a
    // previous attempt, which would cause the DOUBLE_SPEND on this one.
    markCoinSpent(selected.coinIdHex, Date.now());
    if (msg.toUpperCase().includes("DOUBLE_SPEND")) {
      throw new Error(
        "Funding coin was already spent / still in the mempool. Try again to select a different coin."
      );
    }
    throw err;
  }

  // Mark funding coin as pending-spent so an immediate retry uses a different coin.
  markCoinSpent(selected.coinIdHex, Date.now());

  // 7. Compute coin id for the new singleton's coin
  const currentCoinIdHex = await coinId(newStore.coin);
  const launcherIdHex = "0x" + bytesToHex(newStore.launcherId);

  // Persist a pending registry entry immediately so it shows up in the UI
  // while we wait for the chain to confirm it.
  const dataStoreJson = dataStoreToRegistryJson(newStore);
  const baseEntry: RegistryEntry = {
    launcherId: launcherIdHex,
    label: params.label ?? "",
    dataStoreJson,
    ownerSyntheticPkHex: selected.syntheticPkHex,
    currentCoinIdHex,
    status: "pending",
    history: [{ ts: Date.now(), op: "mint" }],
  };
  upsertStore(baseEntry);

  // Wait for the new singleton coin to be confirmed on-chain.
  onStatus?.("Pushed; waiting for on-chain confirmation…");
  await waitForCoinConfirmation(currentCoinIdHex);

  upsertStore({ ...baseEntry, status: "confirmed" });
  onStatus?.("Confirmed");

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
  /** Optional fee in mojos to attach to the spend bundle via addFee. */
  feeMojos?: bigint;
}

/**
 * Update the metadata of an existing DataLayer store.
 *
 * Uses the owner synthetic key stored in the registry for signing.
 * If feeMojos > 0, picks a separate XCH coin and attaches it via wasm.addFee,
 * linked to the singleton spend via assertCoinIds.
 * The registry entry is updated with the returned `newStore`.
 */
export async function updateMetadata(
  launcherId: string,
  params: UpdateMetadataParams,
  onStatus?: (s: string) => void
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

  const updateCoinSpends = result.coinSpends as WasmCoinSpend[];
  const newStore = result.newStore as DataStoreWasm;

  // Attach fee if requested
  const feeMojos = params.feeMojos ?? 0n;
  let allCoinSpends: WasmCoinSpend[] = updateCoinSpends;
  let feeSel: PickedCoin | null = null;

  if (feeMojos > 0n) {
    feeSel = await pickXchCoin(feeMojos);
    if (!feeSel) {
      throw new Error(
        `No spendable XCH coin found to pay the ${feeMojos} mojo fee. ` +
          "Please fund the wallet and try again."
      );
    }

    // The singleton coin being spent in this update
    const assertId = hex0xToBytes(entry.currentCoinIdHex);

    const feeCoinSpends = wasm.addFee(
      hex0xToBytes(feeSel.syntheticPkHex),
      [feeSel.coin] as unknown as object,
      [assertId] as unknown as object,
      feeMojos
    ) as WasmCoinSpend[];

    allCoinSpends = [...updateCoinSpends, ...feeCoinSpends];
  }

  const wcCoinSpends = allCoinSpends.map(coinSpendToWallet);
  const aggSig = assertAggSigHex(await signCoinSpends(wcCoinSpends, false, false) ?? "");
  if (!aggSig) throw new Error("Sage rejected signing or returned no signature.");

  onStatus?.("Pushing spend bundle…");
  const spendBundle = toSpendBundleJson(allCoinSpends, aggSig);

  try {
    await pushTx(spendBundle);
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    if (feeSel) markCoinSpent(feeSel.coinIdHex, Date.now());
    if (msg.toUpperCase().includes("DOUBLE_SPEND")) {
      throw new Error(
        "Fee coin was already spent / still in the mempool. Try again to select a different coin."
      );
    }
    throw err;
  }

  if (feeSel) markCoinSpent(feeSel.coinIdHex, Date.now());

  const currentCoinIdHex = await coinId(newStore.coin);
  const now = Date.now();

  // Persist the new store state as pending, then wait for the NEW store
  // coin to be confirmed on-chain before marking it confirmed.
  const updatedEntry: RegistryEntry = {
    ...entry,
    label: params.newLabel ?? entry.label,
    dataStoreJson: dataStoreToRegistryJson(newStore),
    currentCoinIdHex,
    status: "pending",
    history: [
      ...entry.history,
      { ts: now, op: "updateMetadata" },
    ],
  };
  upsertStore(updatedEntry);

  onStatus?.("Pushed; waiting for on-chain confirmation…");
  await waitForCoinConfirmation(currentCoinIdHex);

  upsertStore({ ...updatedEntry, status: "confirmed" });
  onStatus?.("Confirmed");
}

// ---------------------------------------------------------------------------
// del (melt)
// ---------------------------------------------------------------------------

/**
 * Melt (permanently delete) a DataLayer store.
 *
 * If feeMojos > 0, picks a separate XCH coin and attaches it via wasm.addFee,
 * linked to the singleton melt via assertCoinIds.
 * On success the registry entry is marked "deleted".
 */
export async function del(
  launcherId: string,
  feeMojos: bigint,
  onStatus?: (s: string) => void
): Promise<void> {
  const entry = getStore(launcherId);
  if (!entry) throw new Error(`Store not found in registry: ${launcherId}`);
  if (entry.status === "deleted")
    throw new Error(`Store ${launcherId} is already deleted.`);

  // The coin we are about to spend — captured BEFORE the melt.
  const spentCoinIdHex = entry.currentCoinIdHex;

  const store = dataStoreFromRegistryJson(entry.dataStoreJson);
  const ownerPublicKey = hex0xToBytes(entry.ownerSyntheticPkHex);

  const wasm = await getWasm();
  const meltCoinSpends = wasm.meltStore(
    store as unknown as object,
    ownerPublicKey
  ) as WasmCoinSpend[];

  // Attach fee if requested
  let allCoinSpends: WasmCoinSpend[] = meltCoinSpends;
  let feeSel: PickedCoin | null = null;

  if (feeMojos > 0n) {
    feeSel = await pickXchCoin(feeMojos);
    if (!feeSel) {
      throw new Error(
        `No spendable XCH coin found to pay the ${feeMojos} mojo fee. ` +
          "Please fund the wallet and try again."
      );
    }

    // Assert concurrent spend of the singleton being melted
    const assertId = hex0xToBytes(entry.currentCoinIdHex);

    const feeCoinSpends = wasm.addFee(
      hex0xToBytes(feeSel.syntheticPkHex),
      [feeSel.coin] as unknown as object,
      [assertId] as unknown as object,
      feeMojos
    ) as WasmCoinSpend[];

    allCoinSpends = [...meltCoinSpends, ...feeCoinSpends];
  }

  const wcCoinSpends = allCoinSpends.map(coinSpendToWallet);
  const aggSig = assertAggSigHex(await signCoinSpends(wcCoinSpends, false, false) ?? "");
  if (!aggSig) throw new Error("Sage rejected signing or returned no signature.");

  onStatus?.("Pushing melt spend…");
  const spendBundle = toSpendBundleJson(allCoinSpends, aggSig);

  try {
    await pushTx(spendBundle);
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    if (feeSel) markCoinSpent(feeSel.coinIdHex, Date.now());
    if (msg.toUpperCase().includes("DOUBLE_SPEND")) {
      throw new Error(
        "Fee coin was already spent / still in the mempool. Try again to select a different coin."
      );
    }
    throw err;
  }

  if (feeSel) markCoinSpent(feeSel.coinIdHex, Date.now());

  // Wait until the store coin we just melted is recorded as spent on-chain.
  onStatus?.("Pushed; waiting for on-chain confirmation…");
  await waitForCoinSpent(spentCoinIdHex);

  markDeleted(launcherId, Date.now());
  onStatus?.("Confirmed");
}
