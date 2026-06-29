//! Hand-authored TypeScript interface declarations for the wasm boundary, emitted verbatim into the
//! generated `chip35_dl_coin_wasm.d.ts` via `#[wasm_bindgen(typescript_custom_section)]`.
//!
//! Why this exists: wasm-bindgen types every `JsValue` parameter/return as `any`. The serde boundary
//! structs in [`crate::types`] / [`crate::asset_types`] / [`crate::monetization_types`] already define
//! the real shapes, but they never reach TypeScript. This module re-declares those shapes as exported
//! TS interfaces so a consumer/agent gets concrete types, discriminated unions for the "exactly one of"
//! invariants, and the documented encoding rules — without adopting tsify. The per-function signatures
//! reference these via `unchecked_param_type` / `unchecked_return_type` annotations in `lib.rs`.
//!
//! KEEP IN LOCKSTEP with the serde structs: these interfaces are hand-mirrored from the Rust shapes,
//! so a field change to a boundary struct must be reflected here in the same change.
//!
//! ## Encoding rules (apply to every type below)
//! - 32-/48-/96-byte hashes, keys, signatures, coin ids, nonces are `Uint8Array` (raw bytes — NOT hex).
//! - `u64`/amounts are `bigint` (mojo amounts exceed 2^53; the boundary encodes them as BigInt).
//! - object keys are `camelCase`.

use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const TS_TYPES: &'static str = r#"
/** A Chia coin. `amount` is mojos (XCH) or base units (CAT). */
export interface Coin {
  parentCoinInfo: Uint8Array;
  puzzleHash: Uint8Array;
  amount: bigint;
}

/** A coin spend: the coin plus its CLVM puzzle reveal + solution (raw program bytes). */
export interface CoinSpend {
  coin: Coin;
  puzzleReveal: Uint8Array;
  solution: Uint8Array;
}

/** A singleton lineage proof (the coin descends from a prior singleton coin). */
export interface LineageProof {
  parentParentCoinInfo: Uint8Array;
  parentInnerPuzzleHash: Uint8Array;
  parentAmount: bigint;
}

/** An eve proof (the coin descends directly from its launcher). */
export interface EveProof {
  parentParentCoinInfo: Uint8Array;
  parentAmount: bigint;
}

/**
 * A singleton proof. Exactly one of `lineageProof` / `eveProof` is set
 * (a discriminated "exactly one of" pair).
 */
export type Proof =
  | { lineageProof: LineageProof; eveProof?: undefined }
  | { lineageProof?: undefined; eveProof: EveProof };

/** A DataStore's metadata (root hash + optional human label/description + size). */
export interface DataStoreMetadata {
  rootHash: Uint8Array;
  label?: string | null;
  description?: string | null;
  bytes?: bigint | null;
  /** 32-byte CLVM program (size-proof) hash, if any. */
  programHash?: Uint8Array | null;
}

/**
 * A delegated puzzle entry on a store's delegated-puzzle set. Exactly one role is populated:
 * `adminInnerPuzzleHash` (admin), `writerInnerPuzzleHash` (writer / deploy token), or the
 * `oraclePaymentPuzzleHash` + `oracleFee` pair (oracle).
 */
export type DelegatedPuzzle =
  | { adminInnerPuzzleHash: Uint8Array }
  | { writerInnerPuzzleHash: Uint8Array }
  | { oraclePaymentPuzzleHash: Uint8Array; oracleFee: bigint };

/** A CHIP-0035 DataLayer store singleton. */
export interface DataStore {
  coin: Coin;
  launcherId: Uint8Array;
  proof: Proof;
  metadata: DataStoreMetadata;
  ownerPuzzleHash: Uint8Array;
  delegatedPuzzles: DelegatedPuzzle[];
}

/** The result of a store spend builder: the coin spends to sign + the resulting store state. */
export interface SuccessResponse {
  coinSpends: CoinSpend[];
  newStore: DataStore;
}

/** A spend bundle: coin spends + the aggregated BLS signature (96 bytes). */
export interface SpendBundle {
  coinSpends: CoinSpend[];
  aggregatedSignature: Uint8Array;
}

/** A CHIP-0007 attribute (trait). */
export interface Attribute {
  traitType: string;
  value: string;
}

/** A CHIP-0007 collection reference embedded in item metadata. */
export interface CollectionRef {
  id: string;
  name: string;
  attributes?: Attribute[];
}

/** A CHIP-0007 metadata document (off-chain JSON). */
export interface Chip0007Metadata {
  format?: string;
  name: string;
  description?: string | null;
  sensitiveContent?: boolean;
  collection?: CollectionRef | null;
  attributes?: Attribute[];
  seriesNumber?: bigint | null;
  seriesTotal?: bigint | null;
  mintingTool?: string | null;
}

/** Result of `buildChip0007Metadata`. */
export interface Chip0007MetadataResult {
  json: string;
  metadataHash: Uint8Array;
}

/** The actual bytes (and claimed hashes) `validateChip0007` checks URI↔hash agreement against. */
export interface Chip0007Assets {
  dataBytes?: Uint8Array;
  dataHash?: Uint8Array;
  metadataBytes?: Uint8Array;
  metadataHash?: Uint8Array;
  licenseBytes?: Uint8Array;
  licenseHash?: Uint8Array;
}

/** Result of `validateChip0007`. */
export interface ValidationResult {
  ok: boolean;
  errors: string[];
}

/** A DID attribution (the creator identity a mint is attributed to). */
export interface DidAttribution {
  launcherId: Uint8Array;
  innerPuzzleHash: Uint8Array;
}

/** On-chain NFT media metadata (dig:// + https fallback URIs + computed hashes). */
export interface NftMediaMetadata {
  dataUris?: string[];
  dataHash?: Uint8Array;
  metadataUris?: string[];
  metadataHash?: Uint8Array;
  licenseUris?: string[];
  licenseHash?: Uint8Array;
  editionNumber?: bigint;
  editionTotal?: bigint;
}

/** Parameters to mint a single NFT. */
export interface NftMintParams {
  metadata: NftMediaMetadata;
  p2PuzzleHash: Uint8Array;
  royaltyPuzzleHash: Uint8Array;
  royaltyBasisPoints: number;
  did?: DidAttribution | null;
}

/** Result of `mintNft`. */
export interface NftMintResult {
  coinSpends: CoinSpend[];
  launcherId: Uint8Array;
  nftCoin: Coin;
}

/** Result of `createDid`. */
export interface CreateDidResult {
  coinSpends: CoinSpend[];
  launcherId: Uint8Array;
  innerPuzzleHash: Uint8Array;
  didCoin: Coin;
}

/** Result of `issueCat`. */
export interface IssueCatResult {
  coinSpends: CoinSpend[];
  assetId: Uint8Array;
  catCoins: Coin[];
}

/** A collection definition (for `generateItemMetadata` / `bulkMint`). */
export interface Collection {
  id: string;
  name: string;
  attributes?: Attribute[];
  royaltyPuzzleHash: Uint8Array;
  royaltyBasisPoints: number;
}

/** One item's on-chain media in a parsed traits manifest. */
export interface ManifestMedia {
  dataUris?: string[];
  dataHash?: Uint8Array;
  metadataUris?: string[];
  metadataHash?: Uint8Array;
  licenseUris?: string[];
  licenseHash?: Uint8Array;
}

/** One item in a parsed traits manifest. */
export interface ManifestItem {
  name: string;
  description?: string | null;
  attributes?: Attribute[];
  media: ManifestMedia;
}

/** A DID coin + identifiers (for `bulkMint`). Simple DIDs use `numVerificationsRequired: 1`. */
export interface Did {
  didCoin: Coin;
  proof: Proof;
  launcherId: Uint8Array;
  innerPuzzleHash: Uint8Array;
  recoveryListHash?: Uint8Array | null;
  numVerificationsRequired?: bigint;
}

/** Result of `bulkMint`. */
export interface BulkMintResult {
  coinSpends: CoinSpend[];
  launcherIds: Uint8Array[];
}

/**
 * Which asset a payment settles in. Exactly one of `xch` / `assetId` is set
 * (a discriminated "exactly one of" pair).
 */
export type PaymentAsset =
  | { xch: true; assetId?: undefined }
  | { xch?: false; assetId: Uint8Array };

/** A CAT's on-chain info. */
export interface CatInfo {
  assetId: Uint8Array;
  hiddenPuzzleHash?: Uint8Array | null;
  p2PuzzleHash: Uint8Array;
}

/** A buyer's CAT coin (as `chip0002_getAssetCoins` returns it) for `buildCatPayment` / `buildDigStorePayment`. */
export interface Cat {
  coin: Coin;
  lineageProof?: LineageProof | null;
  info: CatInfo;
}

/**
 * The cross-system $DIG-payment constants (mainnet), returned by `digConstants()`. Minting a store is
 * FREE of $DIG; a CAPSULE (commit / root-advance) pays the per-capsule price in $DIG to
 * `treasuryInnerPuzzleHash`. Byte-identical across the ecosystem (digstore-chain / hub).
 */
export interface DigConstants {
  /** The DIG CAT asset id (TAIL hash), 32 bytes. */
  assetId: Uint8Array;
  /** The DIG treasury's inner (standard) puzzle hash every per-capsule payment settles to, 32 bytes. */
  treasuryInnerPuzzleHash: Uint8Array;
}

/** The verifiable description of a payment's on-chain commitment. */
export interface PaymentReceipt {
  ownerPuzzleHash: Uint8Array;
  amount: bigint;
  asset: PaymentAsset;
  nonce: Uint8Array;
  paymentCoin: Coin;
}

/** Result of `buildPayment` / `buildCatPayment`. */
export interface PaymentResponse {
  coinSpends: CoinSpend[];
  receipt: PaymentReceipt;
}

/** What an observed payment looks like to the paywall after reading the chain (for `verifyPaymentReceipt`). */
export interface ObservedPayment {
  paidToPuzzleHash: Uint8Array;
  amount: bigint;
  asset: PaymentAsset;
  nonce?: Uint8Array | null;
}

/**
 * Result of `verifyPaymentReceipt`. On denial, `code` is a stable `PaywallError`
 * code (`WRONG_RECIPIENT` | `INSUFFICIENT_AMOUNT` | `WRONG_ASSET` | `NONCE_MISMATCH`).
 */
export interface PaywallResult {
  ok: boolean;
  code?: ChipErrorCode;
  error?: string;
}

/** The on-chain ownership facts a gating proof establishes about an NFT. */
export interface NftOwnershipProof {
  launcherId: Uint8Array;
  ownerPuzzleHash: Uint8Array;
  attributedDid?: Uint8Array | null;
  nftCoinId: Uint8Array;
}

/**
 * Result of the NFT-gating helpers (`proveNftOwnership` / `proveCollectionMembership` /
 * `readNftOwnership`). On failure, `code` is a stable `GatingError` code
 * (`NOT_AN_NFT` | `WRONG_OWNER` | `WRONG_COLLECTION` | `WRONG_NFT`).
 */
export interface GatingResult {
  ok: boolean;
  proof?: NftOwnershipProof;
  code?: ChipErrorCode;
  error?: string;
}

/**
 * Every stable machine error code this module can surface — thrown as `{ code, message }` from a
 * failing export, or carried as the `code` field of a `{ ok:false, code, error }` result. Branch on
 * the code, never the human `message`/`error` string.
 */
export type ChipErrorCode =
  | "INVALID_ARGUMENT"
  | "SERDE_ERROR"
  | "DRIVER_ERROR"
  | "PARSE_ERROR"
  | "PERMISSION_DENIED"
  | "METADATA_ERROR"
  | "NOT_AN_NFT"
  | "WRONG_OWNER"
  | "WRONG_COLLECTION"
  | "WRONG_NFT"
  | "WRONG_RECIPIENT"
  | "INSUFFICIENT_AMOUNT"
  | "WRONG_ASSET"
  | "NONCE_MISMATCH";

/** The structured error a failing export throws: a stable machine `code` beside the human `message`. */
export interface ChipError {
  code: ChipErrorCode;
  message: string;
}

/** Runtime self-description returned by `capabilities()`. */
export interface Capabilities {
  /** The published npm package name. */
  name: string;
  /** The package version (= `version()`). */
  version: string;
  /** Every exported builder/helper (camelCase JS names). */
  builders: string[];
  /** The catalogue of stable machine error codes (see `ChipErrorCode`). */
  errorCodes: ChipErrorCode[];
}
"#;
