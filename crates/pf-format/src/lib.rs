//! PFBinary wire format: tags, header, version.

pub mod header;
pub mod tags;
pub mod version;

pub use header::{
    checksum_bytes, parse_header, validate_flags, write_base_header, Header, HeaderError,
    CHECKSUM_LEN, FLAGS_DEFAULT, FLAGS_RESERVED_MASK, FLAG_CHECKSUM, FLAG_STRDEDUP, HEADER_BASE_LEN,
    MAGIC,
};
pub use tags::*;
pub use version::VERSION_V1;
