use chia_sdk_driver::DriverError;
use thiserror::Error as ThisError;

/// Errors produced by the DataLayer store spend builders.
#[derive(Debug, ThisError)]
pub enum Error {
    #[error("{0:?}")]
    Driver(#[from] DriverError),

    #[error("ParseError: {0}")]
    Parse(String),

    #[error("Permission error: puzzle can't perform this action")]
    Permission,
}

/// Alias so the copied driver bodies (which reference `WalletError`) compile unchanged.
pub type WalletError = Error;
