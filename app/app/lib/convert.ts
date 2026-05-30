// ============================================================================
// convert.ts — pure conversion helpers between chip35 wasm types and JSON
// ============================================================================
//
// MODULE: lib/convert
// PURPOSE: Bridge between the wasm-native types (Uint8Array, bigint) and
//          JSON-safe representations (hex strings, string amounts) for
//          localStorage, WalletConnect wire format, and coinset.org.
//
// DESIGN: No I/O, no wasm calls, no async. Pure data transformation.

// ---------------------------------------------------------------------------
// Basic hex utilities
// ---------------------------------------------------------------------------

/** Convert a Uint8Array to a lowercase hex string (no 0x prefix). */
export function bytesToHex(u8: Uint8Array): string {
  let s = "";
  for (let i = 0; i < u8.length; i++) {
    s += u8[i].toString(16).padStart(2, "0");
  }
  return s;
}

/** Convert a Uint8Array to a `0x`-prefixed lowercase hex string. */
export function bytesToHex0x(u8: Uint8Array): string {
  return "0x" + bytesToHex(u8);
}

/**
 * Decode a hex string (with or without `0x` prefix) to a Uint8Array.
 * Throws if the string is not valid hex or has odd length.
 */
export function hex0xToBytes(s: string): Uint8Array {
  const clean = s.replace(/^0x/i, "");
  if (clean.length % 2 !== 0) {
    throw new Error(`hex0xToBytes: odd-length hex string: "${s}"`);
  }
  const out = new Uint8Array(clean.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(clean.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

// ---------------------------------------------------------------------------
// WalletConnect wire format helpers
// ---------------------------------------------------------------------------

import type { WcCoinSpend, WcCoin } from "./walletConnect";

/**
 * Wasm `CoinSpend` (Uint8Array + bigint) → WalletConnect wire format
 * (snake_case, `0x`-prefixed hex, amount as Number).
 */
export function coinSpendToWallet(cs: {
  coin: { parentCoinInfo: Uint8Array; puzzleHash: Uint8Array; amount: bigint };
  puzzleReveal: Uint8Array;
  solution: Uint8Array;
}): WcCoinSpend {
  return {
    coin: {
      parent_coin_info: bytesToHex0x(cs.coin.parentCoinInfo),
      puzzle_hash: bytesToHex0x(cs.coin.puzzleHash),
      amount: Number(cs.coin.amount),
    },
    puzzle_reveal: bytesToHex0x(cs.puzzleReveal),
    solution: bytesToHex0x(cs.solution),
  };
}

/** Convert an array of wasm CoinSpends + an aggregated signature hex to a spend bundle JSON object. */
export function toSpendBundleJson(
  coinSpends: Array<{
    coin: { parentCoinInfo: Uint8Array; puzzleHash: Uint8Array; amount: bigint };
    puzzleReveal: Uint8Array;
    solution: Uint8Array;
  }>,
  aggSigHex: string
): {
  coin_spends: WcCoinSpend[];
  aggregated_signature: string;
} {
  const normalizedSig = aggSigHex.startsWith("0x")
    ? aggSigHex
    : "0x" + aggSigHex;
  return {
    coin_spends: coinSpends.map(coinSpendToWallet),
    aggregated_signature: normalizedSig,
  };
}

// ---------------------------------------------------------------------------
// DataStore JSON shapes for localStorage
// ---------------------------------------------------------------------------

/** JSON-safe shape for a DataStore (all bytes as hex strings, bigints as decimal strings). */
export interface DataStoreJson {
  coin: {
    parentCoinInfo: string; // 0x-hex
    puzzleHash: string; // 0x-hex
    amount: string; // decimal string (bigint)
  };
  launcherId: string; // 0x-hex
  proof: {
    lineageProof?: {
      parentParentCoinInfo: string; // 0x-hex
      parentInnerPuzzleHash: string; // 0x-hex
      parentAmount: string; // decimal string
    };
    eveProof?: {
      parentParentCoinInfo: string; // 0x-hex
      parentAmount: string; // decimal string
    };
  };
  metadata: {
    rootHash: string; // 0x-hex
    label?: string;
    description?: string;
    bytes?: string; // decimal string
    programHash?: string; // 0x-hex
  };
  ownerPuzzleHash: string; // 0x-hex
  delegatedPuzzles: Array<{
    adminInnerPuzzleHash?: string; // 0x-hex
    writerInnerPuzzleHash?: string; // 0x-hex
    oraclePaymentPuzzleHash?: string; // 0x-hex
    oracleFee?: string; // decimal string
  }>;
}

/** Wasm DataStore native shape (matches chip35-dl-coin-wasm types). */
export interface DataStoreWasm {
  coin: { parentCoinInfo: Uint8Array; puzzleHash: Uint8Array; amount: bigint };
  launcherId: Uint8Array;
  proof: {
    lineageProof?: {
      parentParentCoinInfo: Uint8Array;
      parentInnerPuzzleHash: Uint8Array;
      parentAmount: bigint;
    };
    eveProof?: {
      parentParentCoinInfo: Uint8Array;
      parentAmount: bigint;
    };
  };
  metadata: {
    rootHash: Uint8Array;
    label?: string;
    description?: string;
    bytes?: bigint;
    programHash?: Uint8Array;
  };
  ownerPuzzleHash: Uint8Array;
  delegatedPuzzles: Array<{
    adminInnerPuzzleHash?: Uint8Array;
    writerInnerPuzzleHash?: Uint8Array;
    oraclePaymentPuzzleHash?: Uint8Array;
    oracleFee?: bigint;
  }>;
}

/** Convert a wasm-native DataStore to a JSON-safe form for localStorage. */
export function dataStoreToRegistryJson(ds: DataStoreWasm): DataStoreJson {
  return {
    coin: {
      parentCoinInfo: bytesToHex0x(ds.coin.parentCoinInfo),
      puzzleHash: bytesToHex0x(ds.coin.puzzleHash),
      amount: ds.coin.amount.toString(),
    },
    launcherId: bytesToHex0x(ds.launcherId),
    proof: {
      lineageProof: ds.proof.lineageProof
        ? {
            parentParentCoinInfo: bytesToHex0x(
              ds.proof.lineageProof.parentParentCoinInfo
            ),
            parentInnerPuzzleHash: bytesToHex0x(
              ds.proof.lineageProof.parentInnerPuzzleHash
            ),
            parentAmount: ds.proof.lineageProof.parentAmount.toString(),
          }
        : undefined,
      eveProof: ds.proof.eveProof
        ? {
            parentParentCoinInfo: bytesToHex0x(
              ds.proof.eveProof.parentParentCoinInfo
            ),
            parentAmount: ds.proof.eveProof.parentAmount.toString(),
          }
        : undefined,
    },
    metadata: {
      rootHash: bytesToHex0x(ds.metadata.rootHash),
      label: ds.metadata.label,
      description: ds.metadata.description,
      bytes: ds.metadata.bytes !== undefined ? ds.metadata.bytes.toString() : undefined,
      programHash:
        ds.metadata.programHash !== undefined
          ? bytesToHex0x(ds.metadata.programHash)
          : undefined,
    },
    ownerPuzzleHash: bytesToHex0x(ds.ownerPuzzleHash),
    delegatedPuzzles: ds.delegatedPuzzles.map((dp) => ({
      adminInnerPuzzleHash: dp.adminInnerPuzzleHash
        ? bytesToHex0x(dp.adminInnerPuzzleHash)
        : undefined,
      writerInnerPuzzleHash: dp.writerInnerPuzzleHash
        ? bytesToHex0x(dp.writerInnerPuzzleHash)
        : undefined,
      oraclePaymentPuzzleHash: dp.oraclePaymentPuzzleHash
        ? bytesToHex0x(dp.oraclePaymentPuzzleHash)
        : undefined,
      oracleFee:
        dp.oracleFee !== undefined ? dp.oracleFee.toString() : undefined,
    })),
  };
}

/**
 * Convert a JSON-safe DataStore back to the exact wasm-native shape
 * that `updateStoreMetadata` / `meltStore` / `updateStoreOwnership` expect.
 */
export function dataStoreFromRegistryJson(j: DataStoreJson): DataStoreWasm {
  return {
    coin: {
      parentCoinInfo: hex0xToBytes(j.coin.parentCoinInfo),
      puzzleHash: hex0xToBytes(j.coin.puzzleHash),
      amount: BigInt(j.coin.amount),
    },
    launcherId: hex0xToBytes(j.launcherId),
    proof: {
      lineageProof: j.proof.lineageProof
        ? {
            parentParentCoinInfo: hex0xToBytes(
              j.proof.lineageProof.parentParentCoinInfo
            ),
            parentInnerPuzzleHash: hex0xToBytes(
              j.proof.lineageProof.parentInnerPuzzleHash
            ),
            parentAmount: BigInt(j.proof.lineageProof.parentAmount),
          }
        : undefined,
      eveProof: j.proof.eveProof
        ? {
            parentParentCoinInfo: hex0xToBytes(
              j.proof.eveProof.parentParentCoinInfo
            ),
            parentAmount: BigInt(j.proof.eveProof.parentAmount),
          }
        : undefined,
    },
    metadata: {
      rootHash: hex0xToBytes(j.metadata.rootHash),
      label: j.metadata.label,
      description: j.metadata.description,
      bytes:
        j.metadata.bytes !== undefined ? BigInt(j.metadata.bytes) : undefined,
      programHash:
        j.metadata.programHash !== undefined
          ? hex0xToBytes(j.metadata.programHash)
          : undefined,
    },
    ownerPuzzleHash: hex0xToBytes(j.ownerPuzzleHash),
    delegatedPuzzles: j.delegatedPuzzles.map((dp) => ({
      adminInnerPuzzleHash: dp.adminInnerPuzzleHash
        ? hex0xToBytes(dp.adminInnerPuzzleHash)
        : undefined,
      writerInnerPuzzleHash: dp.writerInnerPuzzleHash
        ? hex0xToBytes(dp.writerInnerPuzzleHash)
        : undefined,
      oraclePaymentPuzzleHash: dp.oraclePaymentPuzzleHash
        ? hex0xToBytes(dp.oraclePaymentPuzzleHash)
        : undefined,
      oracleFee:
        dp.oracleFee !== undefined ? BigInt(dp.oracleFee) : undefined,
    })),
  };
}

// ---------------------------------------------------------------------------
// Coin id computation
// ---------------------------------------------------------------------------
//
// chip35-dl-coin-wasm does NOT export a getCoinId helper.
//
// APPROACH: We compute it in JS using the Web Crypto API (SHA-256).
// Chia coin id = SHA-256(parent_coin_info ++ puzzle_hash ++ clvm_int(amount))
//
// CLVM int encoding: big-endian minimal-length signed integer.
// For the non-negative amounts seen in practice, this is the same as
// big-endian unsigned with a leading 0x00 byte when the high bit is set.
//
// This is called lazily from storeOps and registry, never at import time.

/**
 * CLVM big-endian minimal-length signed integer encoding for coin amounts.
 * Amounts are always non-negative u64, so only the positive path is needed:
 * minimal big-endian with a leading 0x00 byte when the high bit is set
 * (correct CLVM positive-int rule).
 */
function clvmEncodeInt(n: bigint): Uint8Array {
  if (n === 0n) return new Uint8Array(0);
  const bytes: number[] = [];
  let v = n;
  while (v > 0n) {
    bytes.unshift(Number(v & 0xffn));
    v >>= 8n;
  }
  // Add a leading 0x00 when the high bit is set to keep the value positive.
  if (bytes[0] & 0x80) {
    bytes.unshift(0x00);
  }
  return new Uint8Array(bytes);
}

/**
 * Compute the Chia coin id (SHA-256 of parent_coin_info ++ puzzle_hash ++ clvm_int(amount))
 * for a wasm Coin. Returns the coin id as a lowercase hex string (no 0x prefix).
 *
 * Uses the Web Crypto API — must be called in a browser context.
 */
export async function coinId(coin: {
  parentCoinInfo: Uint8Array;
  puzzleHash: Uint8Array;
  amount: bigint;
}): Promise<string> {
  const amountBytes = clvmEncodeInt(coin.amount);
  const buf = new Uint8Array(
    coin.parentCoinInfo.length + coin.puzzleHash.length + amountBytes.length
  );
  buf.set(coin.parentCoinInfo, 0);
  buf.set(coin.puzzleHash, coin.parentCoinInfo.length);
  buf.set(amountBytes, coin.parentCoinInfo.length + coin.puzzleHash.length);

  const hashBuf = await crypto.subtle.digest("SHA-256", buf);
  const hashBytes = new Uint8Array(hashBuf);
  return bytesToHex(hashBytes);
}

/** Convenience: wasm Coin → WcCoin wire shape (for reference/display). */
export function coinToWc(c: {
  parentCoinInfo: Uint8Array;
  puzzleHash: Uint8Array;
  amount: bigint;
}): WcCoin {
  return {
    parent_coin_info: bytesToHex0x(c.parentCoinInfo),
    puzzle_hash: bytesToHex0x(c.puzzleHash),
    amount: Number(c.amount),
  };
}
