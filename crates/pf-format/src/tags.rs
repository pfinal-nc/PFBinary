//! PFBinary tag constants (protocol v1).

/// `NULL`
pub const TAG_NULL: u8 = 0x00;
/// `FALSE`
pub const TAG_FALSE: u8 = 0x01;
/// `TRUE`
pub const TAG_TRUE: u8 = 0x02;
/// `VARINT` — zigzag + LEB128 i64
pub const TAG_VARINT: u8 = 0x03;
/// `DOUBLE` — IEEE-754 little-endian
pub const TAG_DOUBLE: u8 = 0x04;
/// `STR_EMPTY`
pub const TAG_STR_EMPTY: u8 = 0x05;
/// `STR_VARINT` — uvarint(len) + bytes
pub const TAG_STR_VARINT: u8 = 0x06;
/// `STR_REF` — uvarint(string_id)
pub const TAG_STR_REF: u8 = 0x07;
/// `ARRAY_PACKED` — uvarint(count) + values
pub const TAG_ARRAY_PACKED: u8 = 0x08;
/// `ARRAY_HASH` — uvarint(count) + (key, value)*
pub const TAG_ARRAY_HASH: u8 = 0x09;
/// `PACKED_LONGS` — uvarint(count) + count × zigzag-varint (no per-element tags)
pub const TAG_PACKED_LONGS: u8 = 0x0A;
/// `ARRAY_ROWS` — uvarint(nrows) + uvarint(ncols) + ncols×Key + nrows×(ncols×Value)
/// Homogeneous list of hash rows sharing one key schema (keys written once).
pub const TAG_ARRAY_ROWS: u8 = 0x0B;
/// `ARRAY_REPEAT` — uvarint(count) + Value — packed list of `count` identical values.
pub const TAG_ARRAY_REPEAT: u8 = 0x0C;

/// First reserved tag for further V1.x extensions.
pub const TAG_V1_EXT_START: u8 = 0x0D;
/// First reserved tag for V2 (OBJECT / REF / …).
pub const TAG_V2_START: u8 = 0x40;

/// Returns true if `tag` is a defined V1 value tag.
#[inline]
pub fn is_v1_value_tag(tag: u8) -> bool {
    tag <= TAG_ARRAY_REPEAT
}

/// Returns true if `tag` is a legal ARRAY_HASH key tag.
#[inline]
pub fn is_v1_key_tag(tag: u8) -> bool {
    matches!(
        tag,
        TAG_VARINT | TAG_STR_EMPTY | TAG_STR_VARINT | TAG_STR_REF
    )
}
