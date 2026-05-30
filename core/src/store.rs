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
                        let hint = ctx.hint(DATASTORE_LAUNCHER_HINT)?;

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

#[derive(Clone, Debug)]
pub enum DataStoreInnerSpend {
    Owner(PublicKey),
    Admin(PublicKey),
    Writer(PublicKey),
    // does not include oracle since it can't change metadata/owners
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

pub fn melt_store(datastore: DataStore, owner_pk: PublicKey) -> Result<Vec<CoinSpend>, WalletError> {
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
    let spend_bundle = SpendBundle::from_bytes(&bytes).map_err(|e| Error::Parse(format!("bundle parse: {e}")))?;
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
