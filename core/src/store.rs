//! CHIP-0035 DataLayer store spend builders.
//!
//! Copied from DataLayer-Driver/src/wallet.rs (399-484, 696-929) and
//! src/lib.rs (136-148). The only edits vs. the source: the error type is
//! `crate::Error` (aliased as `WalletError`), and the two serialization
//! helpers map their errors explicitly into `Error::Parse`.

use chia_bls::PublicKey;
use chia_protocol::{Bytes, Bytes32, Coin, CoinSpend, SpendBundle};
use chia_puzzle_types::standard::StandardArgs;
use chia_puzzles::SINGLETON_LAUNCHER_HASH;
use chia_sdk_driver::{
    get_merkle_tree, DataStore, DataStoreMetadata, DelegatedPuzzle, DriverError, Launcher, Layer,
    OracleLayer, SpendContext, SpendWithConditions, StandardLayer, WriterLayer,
};
use chia_sdk_types::{
    announcement_id,
    conditions::{CreateCoin, MeltSingleton, UpdateDataStoreMerkleRoot},
    Condition, Conditions,
};
use clvm_traits::ToClvm;
use hex_literal::hex;

use crate::error::{Error, WalletError};
use crate::types::SuccessResponse;

/* echo -n 'datastore' | sha256sum */
pub const DATASTORE_LAUNCHER_HINT: Bytes32 = Bytes32::new(hex!(
    "aa7e5b234e1d55967bf0a316395a2eab6cb3370332c0f251f0e44a5afb84fc68"
));

/// Domain tag for the digstore-scoped owner DISCOVERY hint. Scoping the hint to digstores
/// (rather than hinting the raw owner puzzle hash) means a coinset
/// `get_coin_records_by_hint(digstore_owner_hint(owner_ph))` query returns ONLY the owner's
/// DataLayer store launcher coins — never their ordinary XCH coins, which are hinted with
/// the raw puzzle hash. Versioned so the derivation can evolve without ambiguity.
///
/// This enables OWNER DISCOVERY BY CAPSULE LINEAGE. A capsule is one immutable store
/// generation = the pair `(storeId, rootHash)` (written `storeId:rootHash`); a store is a
/// sequence of capsules, one per commit. Each launcher coin returned by the hint query is a
/// store's genesis launcher = its FIRST capsule `(storeId, rootHash_0)`, from which the
/// singleton lineage (the store's full sequence of capsules) is followed forward.
///
/// CONTRACT: these bytes (`b"dig:datastore:owner:v1"`) are byte-identical across
/// chip35_dl_coin and digstore — a mutual byte-identical contract. A mismatched hint silently
/// causes enumeration misses (owned stores never surface in the query).
pub const DIGSTORE_OWNER_HINT_DOMAIN: &[u8] = b"dig:datastore:owner:v1";

/// Derive the digstore-scoped owner discovery hint = `sha256(DOMAIN || owner_puzzle_hash)`.
/// It is emitted as the FIRST (indexed) memo on the launch CREATE_COIN so the store is
/// discoverable on-chain by owner. The client (JS) and the `digstore` CLI MUST compute this
/// identically (same domain tag, same byte order) or detection misses stores.
///
/// This is what ENABLES OWNER DISCOVERY BY CAPSULE LINEAGE: querying coinset by this hint
/// yields each owned store's genesis launcher coin = the store's FIRST capsule
/// `(storeId, rootHash_0)` (a capsule = one immutable store generation, the pair
/// `(storeId, rootHash)`). From that genesis the singleton lineage — the store's full sequence
/// of capsules, one per commit — is followed forward.
///
/// This is the SAME derivation `digstore` uses: the domain [`DIGSTORE_OWNER_HINT_DOMAIN`]
/// (`b"dig:datastore:owner:v1"`) is byte-identical across chip35_dl_coin and digstore (a mutual
/// byte-identical contract). A mismatched hint causes enumeration misses.
pub fn digstore_owner_hint(owner_puzzle_hash: Bytes32) -> Bytes32 {
    let mut h = chia_sha2::Sha256::new();
    h.update(DIGSTORE_OWNER_HINT_DOMAIN);
    h.update(owner_puzzle_hash);
    Bytes32::new(h.finalize())
}

// ---------------------------------------------------------------------------
// Delegated-puzzle constructors (the on-chain primitive for hub Teams #43 and
// revocable deploy tokens #17).
//
// A DataStore singleton carries a list of `DelegatedPuzzle`s beside its owner.
// Each grants a delegate one of three roles:
//   - Admin  — update the store AND change delegation (add/remove admins/writers).
//   - Writer — create new generations (advance the root = deploy) but NOT change
//              delegation. A revocable deploy token (#17) IS a writer delegate.
//   - Oracle — anyone may spend for a fixed fee.
//
// The owner grants/revokes these by replacing the delegated-puzzle set via
// `update_store_ownership` (the Teams add-member / deploy-token issue+revoke op).
// These three builders mirror DataLayer-Driver's shapes byte-for-byte so a store
// minted here is interchangeable with one minted by DataLayer-Driver.
// ---------------------------------------------------------------------------

/// Build the **Admin** delegated puzzle for a synthetic key (hub Teams #43 — a team admin).
///
/// An admin delegate may update the store's metadata/root AND change the delegated-puzzle set
/// (add/remove admins and writers — i.e. revoke a deploy token), but cannot transfer ownership
/// outright. The puzzle is curried only with the standard puzzle of `synthetic_key`
/// (`StandardArgs::curry_tree_hash`), so the same key authorizes whatever role the owner granted it.
///
/// Mirrors DataLayer-Driver's `admin_delegated_puzzle_from_key`.
pub fn admin_delegated_puzzle_from_key(synthetic_key: &PublicKey) -> DelegatedPuzzle {
    DelegatedPuzzle::Admin(StandardArgs::curry_tree_hash(*synthetic_key))
}

/// Build the **Writer** delegated puzzle for a synthetic key (a revocable deploy token #17 / a
/// hub Teams writer #43).
///
/// A writer delegate may create new store generations — advance the root, i.e. DEPLOY a new
/// capsule — but may NOT change the delegated-puzzle set or transfer ownership. This is exactly the
/// least-privilege credential a CI deploy bot or a team member needs: it can deploy but can never
/// grant itself more authority. A **deploy token is a writer delegate**; the owner revokes it by
/// replacing the delegated-puzzle set (see [`update_store_ownership`]). The on-chain DIG spend-cap
/// that would further bound a deploy token is the only non-native extra and is future work.
///
/// Mirrors DataLayer-Driver's `writer_delegated_puzzle_from_key`.
pub fn writer_delegated_puzzle_from_key(synthetic_key: &PublicKey) -> DelegatedPuzzle {
    DelegatedPuzzle::Writer(StandardArgs::curry_tree_hash(*synthetic_key))
}

/// Build the **Oracle** delegated puzzle: anyone may spend the store for the fixed `oracle_fee`,
/// paid to `oracle_puzzle_hash`. Unlike admin/writer it is keyed by a payment puzzle hash, not a
/// public key — there is no signer; the fee is the gate.
///
/// Mirrors DataLayer-Driver's `oracle_delegated_puzzle`.
pub fn oracle_delegated_puzzle(oracle_puzzle_hash: Bytes32, oracle_fee: u64) -> DelegatedPuzzle {
    DelegatedPuzzle::Oracle(oracle_puzzle_hash, oracle_fee)
}

/// Build the spend bundle that LAUNCHES a new DataLayer store singleton.
///
/// Spends the minter's `selected_coins` (the first is the lead coin; the rest assert concurrent
/// spend with it), mints the launcher coin via [`Launcher::mint_datastore`], and returns the
/// resulting [`DataStore`] state for chaining into the next update. The launcher CREATE_COIN
/// carries two memos: the digstore-scoped [`digstore_owner_hint`] (first, for owner discovery)
/// then the global [`DATASTORE_LAUNCHER_HINT`]. Any coin value above `fee + 1` mojo is returned
/// to the minter as change, hinted to their own puzzle hash.
///
/// `program_hash` is the optional size-proof field of the metadata. `delegated_puzzles` grants
/// admin/writer/oracle delegated authority on the store.
///
/// # Errors
/// [`WalletError::Parse`] if `selected_coins` is empty; [`WalletError::Driver`] for any
/// underlying spend-construction failure.
#[allow(clippy::too_many_arguments)]
pub fn mint_store(
    minter_synthetic_key: PublicKey,
    selected_coins: Vec<Coin>,
    root_hash: Bytes32,
    label: Option<String>,
    description: Option<String>,
    bytes: Option<u64>,
    program_hash: Option<String>,
    owner_puzzle_hash: Bytes32,
    delegated_puzzles: Vec<DelegatedPuzzle>,
    fee: u64,
) -> Result<SuccessResponse, WalletError> {
    if selected_coins.is_empty() {
        return Err(WalletError::Parse("selected_coins is empty".to_string()));
    }

    let minter_puzzle_hash: Bytes32 = StandardArgs::curry_tree_hash(minter_synthetic_key).into();
    let total_amount_from_coins = selected_coins.iter().map(|c| c.amount).sum::<u64>();

    let total_amount = fee + 1;

    let mut ctx = SpendContext::new();

    let p2 = StandardLayer::new(minter_synthetic_key);

    let lead_coin = selected_coins[0];
    let lead_coin_name = lead_coin.coin_id();

    for coin in selected_coins.into_iter().skip(1) {
        p2.spend(
            &mut ctx,
            coin,
            Conditions::new().assert_concurrent_spend(lead_coin_name),
        )?;
    }

    let (launch_singleton, datastore) = Launcher::new(lead_coin_name, 1).mint_datastore(
        &mut ctx,
        DataStoreMetadata {
            root_hash,
            label,
            description,
            bytes,
            size_proof: program_hash,
        },
        owner_puzzle_hash.into(),
        delegated_puzzles,
    )?;

    let launch_singleton = Conditions::new().extend(
        launch_singleton
            .into_iter()
            .map(|cond| {
                if let Condition::CreateCoin(cc) = cond {
                    if cc.puzzle_hash == SINGLETON_LAUNCHER_HASH.into() {
                        // First (indexed) memo = the digstore-scoped owner discovery hint, so
                        // the launcher coin (coin_id == launcher_id == store_id) is found by
                        // get_coin_records_by_hint(digstore_owner_hint(owner_ph)). The global
                        // DATASTORE_LAUNCHER_HINT is kept as a second memo for compatibility.
                        let hint = ctx.memos(&[
                            digstore_owner_hint(owner_puzzle_hash),
                            DATASTORE_LAUNCHER_HINT,
                        ])?;

                        return Ok(Condition::CreateCoin(CreateCoin {
                            puzzle_hash: cc.puzzle_hash,
                            amount: cc.amount,
                            memos: hint,
                        }));
                    }

                    return Ok(Condition::CreateCoin(cc));
                }

                Ok(cond)
            })
            .collect::<Result<Vec<_>, WalletError>>()?,
    );

    let lead_coin_conditions = if total_amount_from_coins > total_amount {
        let hint = ctx.hint(minter_puzzle_hash)?;

        launch_singleton.create_coin(
            minter_puzzle_hash,
            total_amount_from_coins - total_amount,
            hint,
        )
    } else {
        launch_singleton
    };
    p2.spend(&mut ctx, lead_coin, lead_coin_conditions)?;

    Ok(SuccessResponse {
        coin_spends: ctx.take(),
        new_datastore: datastore,
    })
}

/// Reconstruct a DataStore from the coin spend that CREATED its current coin. For an eve
/// store (no updates) this is the launcher coin's spend; for an updated store it is the
/// latest update spend. Lets a client MELT a store it did not mint in-session (it fetches
/// the creating spend from a full node, then rebuilds the DataStore here). `prev_delegated`
/// is the parent's delegated-puzzle set ([] for an eve/owner-only store). Returns the
/// reconstructed DataStore, or a Parse error if the spend is not a DataStore spend.
pub fn datastore_from_spend(
    parent_spend: CoinSpend,
    prev_delegated: Vec<DelegatedPuzzle>,
) -> Result<DataStore, WalletError> {
    let ctx = &mut SpendContext::new();
    DataStore::<DataStoreMetadata>::from_spend(ctx, &parent_spend, &prev_delegated)?
        .ok_or_else(|| WalletError::Parse("coin spend is not a DataStore spend".to_string()))
}

/// Which delegated role authorizes a store update, carrying that role's public key.
///
/// The role determines what the spend may do: an [`Owner`](Self::Owner) can change metadata
/// and ownership; an [`Admin`](Self::Admin) can update metadata and the delegated-puzzle set; a
/// [`Writer`](Self::Writer) can only update metadata. There is intentionally no `Oracle` variant
/// — oracle spends pay a fee but cannot change metadata or owners.
#[derive(Clone, Debug)]
pub enum DataStoreInnerSpend {
    /// Spend authorized by the store owner (full authority).
    Owner(PublicKey),
    /// Spend authorized by an admin delegated puzzle.
    Admin(PublicKey),
    /// Spend authorized by a writer delegated puzzle (metadata only).
    Writer(PublicKey),
}

fn update_store_with_conditions(
    ctx: &mut SpendContext,
    conditions: Conditions,
    datastore: DataStore,
    inner_spend_info: DataStoreInnerSpend,
    allow_admin: bool,
    allow_writer: bool,
) -> Result<SuccessResponse, WalletError> {
    let inner_datastore_spend = match inner_spend_info {
        DataStoreInnerSpend::Owner(pk) => {
            StandardLayer::new(pk).spend_with_conditions(ctx, conditions)?
        }
        DataStoreInnerSpend::Admin(pk) => {
            if !allow_admin {
                return Err(WalletError::Permission);
            }

            StandardLayer::new(pk).spend_with_conditions(ctx, conditions)?
        }
        DataStoreInnerSpend::Writer(pk) => {
            if !allow_writer {
                return Err(WalletError::Permission);
            }

            WriterLayer::new(StandardLayer::new(pk)).spend(ctx, conditions)?
        }
    };

    let parent_delegated_puzzles = datastore.info.delegated_puzzles.clone();
    let new_spend = datastore.spend(ctx, inner_datastore_spend)?;

    let new_datastore =
        DataStore::<DataStoreMetadata>::from_spend(ctx, &new_spend, &parent_delegated_puzzles)?
            .ok_or(WalletError::Parse("Store from spend is None".to_string()))?;

    Ok(SuccessResponse {
        coin_spends: vec![new_spend],
        new_datastore,
    })
}

/// Build the spend that transfers store ownership and/or replaces its delegated-puzzle set.
///
/// An [`Owner`](DataStoreInnerSpend::Owner) spend re-creates the singleton under
/// `new_owner_puzzle_hash` with `new_delegated_puzzles`. An [`Admin`](DataStoreInnerSpend::Admin)
/// spend cannot move ownership directly, so it instead emits an `UpdateDataStoreMerkleRoot`
/// condition over the new delegated-puzzle set (recreation memos pin the owner). Returns the new
/// [`DataStore`] state.
///
/// # Errors
/// [`WalletError::Permission`] if authorized by a [`Writer`](DataStoreInnerSpend::Writer)
/// (writers cannot change ownership); [`WalletError::Driver`]/[`WalletError::Parse`] on spend
/// construction or re-parse failure.
pub fn update_store_ownership(
    datastore: DataStore,
    new_owner_puzzle_hash: Bytes32,
    new_delegated_puzzles: Vec<DelegatedPuzzle>,
    inner_spend_info: DataStoreInnerSpend,
) -> Result<SuccessResponse, WalletError> {
    let ctx = &mut SpendContext::new();

    let update_condition: Condition = match inner_spend_info {
        DataStoreInnerSpend::Owner(_) => {
            DataStore::<DataStoreMetadata>::owner_create_coin_condition(
                ctx,
                datastore.info.launcher_id,
                new_owner_puzzle_hash,
                new_delegated_puzzles,
                true,
            )?
        }
        DataStoreInnerSpend::Admin(_) => {
            let merkle_tree = get_merkle_tree(ctx, new_delegated_puzzles.clone())?;

            let new_merkle_root_condition = UpdateDataStoreMerkleRoot {
                new_merkle_root: merkle_tree.root(),
                memos: DataStore::<DataStoreMetadata>::get_recreation_memos(
                    datastore.info.launcher_id,
                    new_owner_puzzle_hash.into(),
                    new_delegated_puzzles,
                ),
            }
            .to_clvm(&mut **ctx)
            .map_err(DriverError::ToClvm)?;

            Condition::Other(new_merkle_root_condition)
        }
        _ => return Err(WalletError::Permission),
    };

    let update_conditions = Conditions::new().with(update_condition);

    update_store_with_conditions(
        ctx,
        update_conditions,
        datastore,
        inner_spend_info,
        true,
        false,
    )
}

/// Build the spend that updates a store's metadata (root hash, label, description, size, and
/// optional size-proof `program_hash`).
///
/// Emits a `new_metadata` condition with the supplied fields. An
/// [`Owner`](DataStoreInnerSpend::Owner) spend additionally re-creates the singleton coin under
/// the existing owner and delegated puzzles (so the owner keeps control); admin/writer spends
/// rely on the store's own recreation. Returns the new [`DataStore`] state.
///
/// # Errors
/// [`WalletError::Permission`] if the role is not allowed; [`WalletError::Driver`]/
/// [`WalletError::Parse`] on spend construction or re-parse failure.
pub fn update_store_metadata(
    datastore: DataStore,
    new_root_hash: Bytes32,
    new_label: Option<String>,
    new_description: Option<String>,
    new_bytes: Option<u64>,
    new_program_hash: Option<String>,
    inner_spend_info: DataStoreInnerSpend,
) -> Result<SuccessResponse, WalletError> {
    let ctx = &mut SpendContext::new();

    let new_metadata = DataStoreMetadata {
        root_hash: new_root_hash,
        label: new_label,
        description: new_description,
        bytes: new_bytes,
        size_proof: new_program_hash,
    };
    let mut new_metadata_condition = Conditions::new().with(
        DataStore::<DataStoreMetadata>::new_metadata_condition(ctx, new_metadata)?,
    );

    if let DataStoreInnerSpend::Owner(_) = inner_spend_info {
        new_metadata_condition = new_metadata_condition.with(
            DataStore::<DataStoreMetadata>::owner_create_coin_condition(
                ctx,
                datastore.info.launcher_id,
                datastore.info.owner_puzzle_hash,
                datastore.info.delegated_puzzles.clone(),
                false,
            )?,
        );
    }

    update_store_with_conditions(
        ctx,
        new_metadata_condition,
        datastore,
        inner_spend_info,
        true,
        true,
    )
}

/// Build the spend that BURNS (melts) a store singleton, removing it from the chain.
///
/// Owner-authorized only: the inner spend reserves a 1-mojo fee and emits the `MeltSingleton`
/// condition. Returns the single melt coin spend (no resulting [`DataStore`], since the store
/// ceases to exist).
///
/// # Errors
/// [`WalletError::Driver`] on spend-construction failure.
pub fn melt_store(
    datastore: DataStore,
    owner_pk: PublicKey,
) -> Result<Vec<CoinSpend>, WalletError> {
    let ctx = &mut SpendContext::new();

    let melt_conditions = Conditions::new()
        .with(Condition::reserve_fee(1))
        .with(Condition::Other(
            MeltSingleton {}
                .to_clvm(&mut **ctx)
                .map_err(DriverError::ToClvm)?,
        ));

    let inner_datastore_spend =
        StandardLayer::new(owner_pk).spend_with_conditions(ctx, melt_conditions)?;

    let new_spend = datastore.spend(ctx, inner_datastore_spend)?;

    Ok(vec![new_spend])
}

/// Build the spend that exercises a store's ORACLE delegated puzzle.
///
/// Anyone may oracle-spend a store that carries an [`Oracle`](DelegatedPuzzle::Oracle) delegated
/// puzzle: the spender pays `oracle_fee` (defined by that puzzle) plus the network `fee` from
/// their `selected_coins`, asserts the store's oracle puzzle announcement, and re-creates the
/// singleton unchanged. Change above the total is returned to the spender. Returns the new
/// [`DataStore`] state.
///
/// # Errors
/// [`WalletError::Parse`] if `selected_coins` is empty; [`WalletError::Permission`] if the store
/// has no oracle delegated puzzle; [`WalletError::Driver`] on spend-construction failure
/// (including an odd oracle fee).
pub fn oracle_spend(
    spender_synthetic_key: PublicKey,
    selected_coins: Vec<Coin>,
    datastore: DataStore,
    fee: u64,
) -> Result<SuccessResponse, WalletError> {
    if selected_coins.is_empty() {
        return Err(WalletError::Parse("selected_coins is empty".to_string()));
    }

    let Some(DelegatedPuzzle::Oracle(oracle_ph, oracle_fee)) = datastore
        .info
        .delegated_puzzles
        .iter()
        .find(|dp| matches!(dp, DelegatedPuzzle::Oracle(_, _)))
    else {
        return Err(WalletError::Permission);
    };

    let spender_puzzle_hash: Bytes32 = StandardArgs::curry_tree_hash(spender_synthetic_key).into();

    let total_amount = oracle_fee + fee;

    let ctx = &mut SpendContext::new();

    let p2 = StandardLayer::new(spender_synthetic_key);

    let lead_coin = selected_coins[0];
    let lead_coin_name = lead_coin.coin_id();

    let total_amount_from_coins = selected_coins.iter().map(|c| c.amount).sum::<u64>();
    for coin in selected_coins.into_iter().skip(1) {
        p2.spend(
            ctx,
            coin,
            Conditions::new().assert_concurrent_spend(lead_coin_name),
        )?;
    }

    let assert_oracle_conds = Conditions::new().assert_puzzle_announcement(announcement_id(
        datastore.coin.puzzle_hash,
        Bytes::new("$".into()),
    ));

    let mut lead_coin_conditions = assert_oracle_conds;
    if total_amount_from_coins > total_amount {
        let hint = ctx.hint(spender_puzzle_hash)?;

        lead_coin_conditions = lead_coin_conditions.create_coin(
            spender_puzzle_hash,
            total_amount_from_coins - total_amount,
            hint,
        );
    }
    if fee > 0 {
        lead_coin_conditions = lead_coin_conditions.reserve_fee(fee);
    }
    p2.spend(ctx, lead_coin, lead_coin_conditions)?;

    let inner_datastore_spend = OracleLayer::new(*oracle_ph, *oracle_fee)
        .ok_or(DriverError::OddOracleFee)?
        .construct_spend(ctx, ())?;

    let parent_delegated_puzzles = datastore.info.delegated_puzzles.clone();
    let new_spend = datastore.spend(ctx, inner_datastore_spend)?;

    let new_datastore = DataStore::from_spend(ctx, &new_spend, &parent_delegated_puzzles)?
        .ok_or(WalletError::Parse("Store from spend is None".to_string()))?;
    ctx.insert(new_spend.clone());

    Ok(SuccessResponse {
        coin_spends: ctx.take(),
        new_datastore,
    })
}

/// Build coin spends that reserve `fee` mojos from the spender's own XCH coins,
/// asserting concurrent spend of `coin_ids` (e.g. the singleton being updated/melted),
/// returning change to the spender. Lets a singleton-only op (update/melt) carry a fee.
pub fn add_fee(
    spender_synthetic_key: PublicKey,
    selected_coins: Vec<Coin>,
    coin_ids: Vec<Bytes32>,
    fee: u64,
) -> Result<Vec<CoinSpend>, WalletError> {
    if selected_coins.is_empty() {
        return Err(WalletError::Parse("selected_coins is empty".to_string()));
    }
    let spender_puzzle_hash: Bytes32 = StandardArgs::curry_tree_hash(spender_synthetic_key).into();
    let total_amount_from_coins = selected_coins.iter().map(|c| c.amount).sum::<u64>();

    let mut ctx = SpendContext::new();
    let p2 = StandardLayer::new(spender_synthetic_key);

    let lead_coin = selected_coins[0];
    let lead_coin_name = lead_coin.coin_id();

    for coin in selected_coins.into_iter().skip(1) {
        p2.spend(
            &mut ctx,
            coin,
            Conditions::new().assert_concurrent_spend(lead_coin_name),
        )?;
    }

    let mut lead_coin_conditions = Conditions::new().reserve_fee(fee);
    if total_amount_from_coins > fee {
        let hint = ctx.hint(spender_puzzle_hash)?;
        lead_coin_conditions = lead_coin_conditions.create_coin(
            spender_puzzle_hash,
            total_amount_from_coins - fee,
            hint,
        );
    }
    for coin_id in coin_ids {
        lead_coin_conditions = lead_coin_conditions.assert_concurrent_spend(coin_id);
    }

    p2.spend(&mut ctx, lead_coin, lead_coin_conditions)?;

    Ok(ctx.take())
}

/// Decode a hex-encoded spend bundle into its coin spends (keyless).
pub fn hex_spend_bundle_to_coin_spends(hex_str: &str) -> Result<Vec<CoinSpend>, Error> {
    use chia_traits::Streamable;
    let bytes = hex::decode(hex_str).map_err(|e| Error::Parse(format!("hex decode: {e}")))?;
    let spend_bundle =
        SpendBundle::from_bytes(&bytes).map_err(|e| Error::Parse(format!("bundle parse: {e}")))?;
    Ok(spend_bundle.coin_spends)
}

/// Serialize a spend bundle to hex (keyless).
pub fn spend_bundle_to_hex(spend_bundle: &SpendBundle) -> Result<String, Error> {
    use chia_traits::Streamable;
    let bytes = spend_bundle
        .to_bytes()
        .map_err(|e| Error::Parse(format!("bundle serialize: {e}")))?;
    Ok(hex::encode(bytes))
}
