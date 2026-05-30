use chia_protocol::CoinSpend;
use chia_sdk_driver::DataStore;

/// Result of a store-creating/updating spend: the coin spends plus the
/// resulting on-chain store state to feed into the next update.
#[derive(Clone, Debug)]
pub struct SuccessResponse {
    pub coin_spends: Vec<CoinSpend>,
    pub new_datastore: DataStore,
}
