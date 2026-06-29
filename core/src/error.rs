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

impl Error {
    /// A stable, machine-readable `UPPER_SNAKE` code an automated caller can branch on instead of
    /// string-matching the human [`Display`](std::fmt::Display) message. The code is part of the
    /// public contract — changing one is a breaking change. Surfaced across the wasm boundary by the
    /// bindings (see `wasm/src/lib.rs`).
    pub fn code(&self) -> &'static str {
        match self {
            Error::Driver(_) => "DRIVER_ERROR",
            Error::Parse(_) => "PARSE_ERROR",
            Error::Permission => "PERMISSION_DENIED",
        }
    }
}

/// Alias so the copied driver bodies (which reference `WalletError`) compile unchanged.
pub type WalletError = Error;
