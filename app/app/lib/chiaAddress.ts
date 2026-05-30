// ============================================================================
// chiaAddress.ts — Chia bech32m address helpers via chia-wallet-sdk-wasm
// ============================================================================
//
// All functions dynamically import `chia-wallet-sdk-wasm` so they are
// safe to call from client handlers / `dynamic({ssr:false})` components.
// NEVER call them at module scope or during SSR.

/**
 * Decode a bech32m wallet address → raw 32-byte puzzle hash.
 * Returns null if the address is invalid.
 */
export async function puzzleHashBytesFromAddress(
  addr: string
): Promise<Uint8Array | null> {
  const chia = await import("chia-wallet-sdk-wasm");
  let decoded: InstanceType<(typeof chia)["Address"]> | undefined;
  try {
    decoded = chia.Address.decode(addr.trim());
    return new Uint8Array(decoded.puzzleHash);
  } catch {
    return null;
  } finally {
    decoded?.free();
  }
}

/**
 * Decode a bech32m wallet address → `0x`-prefixed 64-char lowercase hex.
 * Returns null if the address is invalid.
 */
export async function puzzleHashHexFromAddress(
  addr: string
): Promise<string | null> {
  const chia = await import("chia-wallet-sdk-wasm");
  let decoded: InstanceType<(typeof chia)["Address"]> | undefined;
  try {
    decoded = chia.Address.decode(addr.trim());
    let h = chia.toHex(decoded.puzzleHash);
    h = h.replace(/^0x/i, "").toLowerCase();
    if (!/^[0-9a-f]{64}$/.test(h)) return null;
    return `0x${h}`;
  } catch {
    return null;
  } finally {
    decoded?.free();
  }
}

/**
 * Extract the synthetic public key (48-byte BLS G1 element, 0x-prefixed hex)
 * from a standard p2 puzzle reveal (hex-encoded CLVM serialization).
 *
 * The standard p2 puzzle is curried with exactly one arg: the synthetic_pk.
 * We use `chia-wallet-sdk-wasm`'s `Clvm` to deserialize and uncurry.
 *
 * Returns null if the puzzle is not a standard p2 shape (e.g. CAT wrapper).
 */
export async function syntheticPkHexFromCoinPuzzle(
  puzzleHex: string
): Promise<string | null> {
  const chia = await import("chia-wallet-sdk-wasm");
  const clean = puzzleHex.replace(/^0x/i, "");
  const bytes = new Uint8Array(clean.length / 2);
  for (let i = 0; i < bytes.length; i++) {
    bytes[i] = parseInt(clean.slice(i * 2, i * 2 + 2), 16);
  }

  const clvm = new chia.Clvm();
  try {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const program = (clvm as any).deserialize(bytes);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const curried = (program as any).uncurry();
    if (!curried) return null;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const args = (curried as any).args as any[];
    if (!args || args.length === 0) return null;
    const pkAtom: Uint8Array | undefined = args[0].toAtom();
    if (!pkAtom || pkAtom.length !== 48) return null;
    let s = "0x";
    for (let i = 0; i < pkAtom.length; i++) {
      s += pkAtom[i].toString(16).padStart(2, "0");
    }
    return s.toLowerCase();
  } catch {
    return null;
  }
}

/**
 * Compute the standard (p2) puzzle hash for a Sage synthetic public key.
 *
 * NOTE: `chia-wallet-sdk-wasm`'s `standardPuzzleHash` CONSUMES the
 * `PublicKey` object (`__destroy_into_raw`). Never call `pk.free()` after.
 *
 * @param syntheticPkHex  48-byte BLS G1 synthetic pubkey, with or without 0x.
 * @returns 0x-prefixed 64-char lowercase hex puzzle hash.
 */
export async function standardPuzzleHashHexFromSyntheticPkHex(
  syntheticPkHex: string
): Promise<string> {
  const chia = await import("chia-wallet-sdk-wasm");
  const raw = syntheticPkHex.trim().replace(/^0x/i, "").toLowerCase();
  if (!/^[0-9a-f]{96}$/.test(raw)) {
    throw new Error(
      "Synthetic pubkey must be a 48-byte G1 element (96 hex chars)"
    );
  }
  const pk = chia.PublicKey.fromBytes(chia.fromHex(raw));
  const ph = chia.standardPuzzleHash(pk);
  let h = chia.toHex(ph);
  h = h.replace(/^0x/i, "").toLowerCase();
  if (!/^[0-9a-f]{64}$/.test(h)) {
    throw new Error("Unexpected standard puzzle hash length");
  }
  return `0x${h}`;
}
