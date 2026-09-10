//! PFSerialize pure Rust core: encode / decode PFBinary v1.

pub mod decoder;
pub mod encoder;
pub mod error;
pub mod host;
pub mod string_table;
pub mod value;
pub mod varint;

pub use decoder::{decode, decode_with, DecodeOptions, DEFAULT_MAX_DEPTH, DEFAULT_MAX_SIZE};
pub use encoder::{encode, encode_with, EncodeOptions, Encoder};
pub use error::{Error, Result};
pub use host::{ArrayKey, Host, Scalar};
pub use value::{Key, Value};

#[cfg(any(test, feature = "test-utils"))]
pub mod test_host;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils {
    pub use crate::test_host::TestHost;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_host::TestHost;
    use crate::value::normalize_for_cmp;
    use pf_format::{FLAG_CHECKSUM, FLAG_STRDEDUP};

    fn roundtrip(v: &Value) -> Value {
        let bytes = encode(v).expect("encode");
        let mut host = TestHost::default();
        decode(&bytes, &mut host).expect("decode")
    }

    #[test]
    fn sample_c_packed_exact_hex() {
        let v = Value::ArrayPacked(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(4),
        ]);
        let bytes = encode(&v).unwrap();
        assert_eq!(
            bytes,
            [
                0x50, 0x46, 0x42, 0x4E, 0x01, 0x02, 0x0A, 0x04, 0x02, 0x04, 0x06, 0x08,
            ]
        );
        assert_eq!(roundtrip(&v), v);
    }

    #[test]
    fn sample_a_assoc_roundtrip() {
        let v = Value::map_str(vec![
            ("id", Value::Int(10001)),
            ("name", Value::str("Tom")),
            ("price", Value::Float(99.99)),
            ("active", Value::Bool(true)),
            ("extra", Value::Null),
        ]);
        let out = roundtrip(&v);
        assert_eq!(normalize_for_cmp(&out), normalize_for_cmp(&v));

        let bytes = encode(&v).unwrap();
        let price_bits = 99.99f64.to_bits().to_le_bytes();
        assert!(bytes.windows(8).any(|w| w == price_bits));
    }

    #[test]
    fn sample_b_repeated_strings() {
        let row = |id: i64| {
            Value::map_str(vec![
                ("id", Value::Int(id)),
                ("status", Value::str("active")),
            ])
        };
        let v = Value::ArrayPacked(vec![row(1), row(2), row(3)]);
        let bytes = encode(&v).unwrap();
        let active = b"active";
        let count = bytes
            .windows(active.len())
            .filter(|w| *w == active)
            .count();
        assert_eq!(count, 1, "string table should dedup 'active'");
        assert_eq!(bytes[6], 0x0B, "homogeneous rows should use ARRAY_ROWS");
        let out = roundtrip(&v);
        assert_eq!(normalize_for_cmp(&out), normalize_for_cmp(&v));
    }

    #[test]
    fn array_rows_roundtrip_repeated_keys() {
        let row = Value::map_str(vec![
            ("id", Value::Int(1)),
            ("status", Value::str("active")),
            ("role", Value::str("member")),
        ]);
        let v = Value::ArrayPacked(vec![row.clone(); 50]);
        let bytes = encode(&v).unwrap();
        assert_eq!(bytes[6], 0x0C, "identical rows use ARRAY_REPEAT");
        assert!(bytes.len() < 80, "got {}", bytes.len());
        let out = roundtrip(&v);
        assert_eq!(normalize_for_cmp(&out), normalize_for_cmp(&v));
    }

    #[test]
    fn array_rows_varying_values() {
        let row = |id: i64| {
            Value::map_str(vec![
                ("id", Value::Int(id)),
                ("status", Value::str("active")),
            ])
        };
        let v = Value::ArrayPacked(vec![row(1), row(2), row(3), row(4)]);
        let bytes = encode(&v).unwrap();
        assert_eq!(bytes[6], 0x0B, "varying rows use ARRAY_ROWS");
        let out = roundtrip(&v);
        assert_eq!(normalize_for_cmp(&out), normalize_for_cmp(&v));
    }

    #[test]
    fn scalars_roundtrip() {
        for v in [
            Value::Null,
            Value::Bool(false),
            Value::Bool(true),
            Value::Int(0),
            Value::Int(-1),
            Value::Int(i64::MIN),
            Value::Int(i64::MAX),
            Value::Float(0.0),
            Value::Float(-0.0),
            Value::str(""),
            Value::str("hello"),
            Value::ArrayPacked(vec![]),
            Value::ArrayHash(vec![]),
        ] {
            let out = roundtrip(&v);
            assert_eq!(normalize_for_cmp(&out), normalize_for_cmp(&v), "{v:?}");
        }
    }

    #[test]
    fn checksum_flag_roundtrip() {
        let v = Value::Int(42);
        let bytes = encode_with(
            &v,
            &EncodeOptions {
                flags: FLAG_CHECKSUM | FLAG_STRDEDUP,
            },
        )
        .unwrap();
        let mut host = TestHost::default();
        assert_eq!(decode(&bytes, &mut host).unwrap(), v);

        let mut bad = bytes.clone();
        bad[6] ^= 0xff;
        let mut host = TestHost::default();
        assert_eq!(decode(&bad, &mut host).unwrap_err(), Error::Checksum);
    }

    #[test]
    fn malformed_bad_magic() {
        let mut host = TestHost::default();
        assert_eq!(
            decode(b"XXXX\x01\x02\x00", &mut host).unwrap_err(),
            Error::BadMagic
        );
    }

    #[test]
    fn malformed_unknown_tag() {
        let mut doc = encode(&Value::Null).unwrap();
        *doc.last_mut().unwrap() = 0x0D;
        let mut host = TestHost::default();
        assert_eq!(decode(&doc, &mut host).unwrap_err(), Error::UnknownTag);
    }

    #[test]
    fn malformed_truncated() {
        let mut host = TestHost::default();
        assert_eq!(
            decode(b"PFBN\x01\x02", &mut host).unwrap_err(),
            Error::Truncated
        );
    }

    #[test]
    fn malformed_trailing() {
        let mut doc = encode(&Value::Null).unwrap();
        doc.push(0x00);
        let mut host = TestHost::default();
        assert_eq!(decode(&doc, &mut host).unwrap_err(), Error::Trailing);
    }

    #[test]
    fn malformed_bad_strref() {
        let doc = vec![0x50, 0x46, 0x42, 0x4E, 0x01, 0x02, 0x07, 0x00];
        let mut host = TestHost::default();
        assert_eq!(decode(&doc, &mut host).unwrap_err(), Error::BadStrRef);
    }

    #[test]
    fn malformed_oversized_array_count() {
        let doc = vec![0x50, 0x46, 0x42, 0x4E, 0x01, 0x02, 0x08, 0x64];
        let mut host = TestHost::default();
        assert_eq!(decode(&doc, &mut host).unwrap_err(), Error::Overflow);
    }

    #[test]
    fn malformed_max_depth() {
        let inner = Value::ArrayPacked(vec![Value::Int(1)]);
        let mid = Value::ArrayPacked(vec![inner]);
        let outer = Value::ArrayPacked(vec![mid]);
        let bytes = encode(&outer).unwrap();
        let mut host = TestHost::default();
        let err = decode_with(
            &bytes,
            &mut host,
            &DecodeOptions {
                max_depth: 2,
                max_size: DEFAULT_MAX_SIZE,
                strict_trailing: true,
            },
        )
        .unwrap_err();
        assert_eq!(err, Error::MaxDepth);
    }

    #[test]
    fn malformed_unknown_flags() {
        let mut doc = encode(&Value::Null).unwrap();
        doc[5] |= 0x04;
        let mut host = TestHost::default();
        assert_eq!(decode(&doc, &mut host).unwrap_err(), Error::UnknownFlags);
    }

    #[test]
    fn malformed_bad_key() {
        let doc = vec![0x50, 0x46, 0x42, 0x4E, 0x01, 0x02, 0x09, 0x01, 0x00, 0x00];
        let mut host = TestHost::default();
        assert_eq!(decode(&doc, &mut host).unwrap_err(), Error::BadKey);
    }

    #[test]
    fn malformed_hash_count_needs_two_bytes_each() {
        // ARRAY_HASH count=3 but only 3 remaining bytes (need ≥6)
        let doc = vec![0x50, 0x46, 0x42, 0x4E, 0x01, 0x02, 0x09, 0x03, 0x00, 0x00, 0x00];
        let mut host = TestHost::default();
        assert_eq!(decode(&doc, &mut host).unwrap_err(), Error::Overflow);
    }

    #[test]
    fn reject_str_ref_when_dedup_disabled() {
        // flags=0x00 (no STRDEDUP), payload STR_REF 0
        let doc = vec![0x50, 0x46, 0x42, 0x4E, 0x01, 0x00, 0x07, 0x00];
        let mut host = TestHost::default();
        assert_eq!(decode(&doc, &mut host).unwrap_err(), Error::UnknownTag);
    }

    #[test]
    fn max_size_rejects_large_payload() {
        let v = Value::str("hello world!!!!"); // payload > 8 after header
        let bytes = encode(&v).unwrap();
        let mut host = TestHost::default();
        let err = decode_with(
            &bytes,
            &mut host,
            &DecodeOptions {
                max_depth: 100,
                max_size: 8,
                strict_trailing: true,
            },
        )
        .unwrap_err();
        assert_eq!(err, Error::MaxSize);
    }

    #[test]
    fn max_size_charges_str_ref_materializations() {
        // One string defined, then many STR_REF copies as hash values (not ARRAY_REPEAT).
        let mut pairs = Vec::new();
        for i in 0..11 {
            pairs.push((Key::Int(i), Value::str("abcdefgh"))); // 8 bytes each
        }
        let v = Value::ArrayHash(pairs);
        let bytes = encode(&v).unwrap();
        // First definition 8 + 10 refs * 8 = 88 string bytes charged; allow only 40.
        let mut host = TestHost::default();
        let err = decode_with(
            &bytes,
            &mut host,
            &DecodeOptions {
                max_depth: 100,
                max_size: 40,
                strict_trailing: true,
            },
        )
        .unwrap_err();
        assert_eq!(err, Error::MaxSize);
    }
}
