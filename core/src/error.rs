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

    /// An allowlist-gated claim was rejected at build time because no membership proof was supplied,
    /// or the supplied proof does not prove the claimer's own puzzle hash is in the committed
    /// allowlist root. This is the OFF-CHAIN / builder-side allowlist gate; trustless on-chain
    /// enforcement (a compiled claim puzzle that runs the merkle verify) remains deferred.
    #[error("Allowlist denied: {0}")]
    AllowlistDenied(String),
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
            Error::AllowlistDenied(_) => "ALLOWLIST_DENIED",
        }
    }
}

/// Alias so the copied driver bodies (which reference `WalletError`) compile unchanged.
pub type WalletError = Error;
