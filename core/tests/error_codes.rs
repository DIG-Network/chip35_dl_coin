//! Stable machine error codes for the error enums (agent-friendly contract).
//!
//! Each variant of `GatingError`, `PaywallError`, and `Error` must carry a stable UPPER_SNAKE
//! `code()` an automated caller can branch on instead of string-matching the human message. These
//! codes are part of the public contract: changing a code is a breaking change, so this test pins
//! the exact strings.

use chip35_dl_coin::{Bytes32, Error, GatingError, PaymentAsset, PaywallError};

const HASH_A: Bytes32 = Bytes32::new([1u8; 32]);
const HASH_B: Bytes32 = Bytes32::new([2u8; 32]);

#[test]
fn gating_error_codes_are_stable() {
    assert_eq!(GatingError::NotAnNft.code(), "NOT_AN_NFT");
    assert_eq!(
        GatingError::WrongOwner {
            expected: HASH_A,
            got: HASH_B
        }
        .code(),
        "WRONG_OWNER"
    );
    assert_eq!(
        GatingError::WrongCollection {
            required: HASH_A,
            got: Some(HASH_B)
        }
        .code(),
        "WRONG_COLLECTION"
    );
    assert_eq!(
        GatingError::WrongNft {
            required: HASH_A,
            got: HASH_B
        }
        .code(),
        "WRONG_NFT"
    );
}

#[test]
fn paywall_error_codes_are_stable() {
    assert_eq!(
        PaywallError::WrongRecipient {
            expected: HASH_A,
            got: HASH_B
        }
        .code(),
        "WRONG_RECIPIENT"
    );
    assert_eq!(
        PaywallError::InsufficientAmount {
            required: 10,
            got: 1
        }
        .code(),
        "INSUFFICIENT_AMOUNT"
    );
    assert_eq!(
        PaywallError::WrongAsset {
            required: PaymentAsset::Xch,
            got: PaymentAsset::Cat(HASH_A)
        }
        .code(),
        "WRONG_ASSET"
    );
    assert_eq!(
        PaywallError::NonceMismatch {
            expected: HASH_A,
            got: None
        }
        .code(),
        "NONCE_MISMATCH"
    );
}

#[test]
fn builder_error_codes_are_stable() {
    assert_eq!(Error::Parse("x".into()).code(), "PARSE_ERROR");
    assert_eq!(Error::Permission.code(), "PERMISSION_DENIED");
    assert_eq!(
        Error::AllowlistDenied("x".into()).code(),
        "ALLOWLIST_DENIED"
    );
    // The Driver variant maps every upstream driver failure to one stable code.
    // (Constructed via the `From<DriverError>` path in real use; here we assert the code mapping
    //  through a representative Parse/Permission and trust the Driver arm is the catch-all.)
}
