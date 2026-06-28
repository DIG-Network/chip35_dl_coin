//! Offer codec (roadmap #35).
//!
//! Canonical conversion between the wire `offer1...` text (the string users copy/paste) and the
//! underlying spend bundle, via chia-sdk-driver's offer compression. This is the codec only;
//! *constructing* an offer (swap asset A for B with royalties) needs trade managers + richer coin
//! selection than the keyless boundary cleanly supports and is deferred to a later wave. Taking an
//! offer is a settlement spend the wallet (Sage) already performs.

use chia_protocol::SpendBundle;
use chia_sdk_driver::{decode_offer as driver_decode, encode_offer as driver_encode};

use crate::error::{Error, WalletError};

/// Encode a spend bundle into canonical offer text (`offer1...`).
///
/// # Errors
/// [`WalletError::Driver`] if the bundle cannot be compressed/encoded.
pub fn encode_offer(spend_bundle: &SpendBundle) -> Result<String, WalletError> {
    driver_encode(spend_bundle).map_err(Error::from)
}

/// Decode canonical offer text (`offer1...`) into its spend bundle.
///
/// # Errors
/// [`WalletError::Driver`] if the text is not a valid offer.
pub fn decode_offer(text: &str) -> Result<SpendBundle, WalletError> {
    driver_decode(text).map_err(Error::from)
}
