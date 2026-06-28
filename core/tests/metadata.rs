//! Tests for the CHIP-0007 metadata builder + validator (roadmap #36).

use chip35_dl_coin::{
    sha256, validate_uri_hash, Attribute, Chip0007Metadata, MetadataError, CHIP0007_FORMAT,
};

#[test]
fn new_sets_canonical_format() {
    let md = Chip0007Metadata::new("DIG Punk #1");
    assert_eq!(md.format, CHIP0007_FORMAT);
    assert_eq!(md.name, "DIG Punk #1");
    md.validate_schema().expect("minimal doc is valid");
}

#[test]
fn canonical_json_is_deterministic_and_hash_reproducible() {
    let mut a = Chip0007Metadata::new("Item");
    a.description = Some("hello".into());
    a.attributes = vec![Attribute {
        trait_type: "Background".into(),
        value: "Blue".into(),
    }];

    let mut b = Chip0007Metadata::new("Item");
    b.description = Some("hello".into());
    b.attributes = vec![Attribute {
        trait_type: "Background".into(),
        value: "Blue".into(),
    }];

    assert_eq!(
        a.to_canonical_json().unwrap(),
        b.to_canonical_json().unwrap(),
        "same logical doc => byte-identical JSON"
    );
    assert_eq!(
        a.compute_metadata_hash().unwrap(),
        b.compute_metadata_hash().unwrap(),
        "same doc => same metadata hash"
    );
}

#[test]
fn metadata_hash_equals_sha256_of_canonical_json() {
    let md = Chip0007Metadata::new("Item");
    let json = md.to_canonical_json().unwrap();
    assert_eq!(
        md.compute_metadata_hash().unwrap(),
        sha256(json.as_bytes()),
        "compute_metadata_hash must be sha256 of the canonical JSON"
    );
}

#[test]
fn validate_schema_rejects_bad_format() {
    let mut md = Chip0007Metadata::new("Item");
    md.format = "CHIP-0015".into();
    assert!(matches!(
        md.validate_schema(),
        Err(MetadataError::BadFormat(_))
    ));
}

#[test]
fn validate_schema_rejects_empty_name() {
    let md = Chip0007Metadata::new("   ");
    assert!(matches!(
        md.validate_schema(),
        Err(MetadataError::MissingField("name"))
    ));
}

#[test]
fn validate_schema_rejects_series_overflow() {
    let mut md = Chip0007Metadata::new("Item");
    md.series_number = Some(5);
    md.series_total = Some(3);
    assert!(matches!(
        md.validate_schema(),
        Err(MetadataError::BadSeries {
            number: 5,
            total: 3
        })
    ));
}

#[test]
fn validate_uri_hash_accepts_matching_bytes() {
    let bytes = b"the real media bytes";
    let hash = sha256(bytes);
    validate_uri_hash("data", bytes, hash).expect("matching bytes pass");
}

#[test]
fn validate_uri_hash_rejects_mismatched_bytes() {
    let hash = sha256(b"the real media bytes");
    let err = validate_uri_hash("data", b"different bytes", hash).unwrap_err();
    assert!(matches!(
        err,
        MetadataError::HashMismatch { which: "data", .. }
    ));
}
