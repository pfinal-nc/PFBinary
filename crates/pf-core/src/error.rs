//! Error codes aligned with protocol §10.2 / architecture ABI.

use pf_format::HeaderError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Error {
    BadMagic = 1,
    UnsupportedVersion = 2,
    UnknownFlags = 3,
    Checksum = 4,
    Truncated = 5,
    Trailing = 6,
    UnknownTag = 7,
    BadKey = 8,
    VarInt = 9,
    BadStrRef = 10,
    MaxDepth = 11,
    MaxSize = 12,
    Overflow = 13,
    UnsupportedType = 14,
    Host = 15,
    Internal = 127,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Error::BadMagic => "bad magic",
            Error::UnsupportedVersion => "unsupported version",
            Error::UnknownFlags => "unknown flags",
            Error::Checksum => "checksum mismatch",
            Error::Truncated => "truncated input",
            Error::Trailing => "trailing bytes",
            Error::UnknownTag => "unknown tag",
            Error::BadKey => "bad array key",
            Error::VarInt => "invalid varint",
            Error::BadStrRef => "bad string reference",
            Error::MaxDepth => "max depth exceeded",
            Error::MaxSize => "max size exceeded",
            Error::Overflow => "length/count overflow",
            Error::UnsupportedType => "unsupported type",
            Error::Host => "host callback failed",
            Error::Internal => "internal error",
        };
        f.write_str(s)
    }
}

impl std::error::Error for Error {}

impl From<HeaderError> for Error {
    fn from(e: HeaderError) -> Self {
        match e {
            HeaderError::Truncated => Error::Truncated,
            HeaderError::BadMagic => Error::BadMagic,
            HeaderError::UnsupportedVersion => Error::UnsupportedVersion,
            HeaderError::UnknownFlags => Error::UnknownFlags,
            HeaderError::Checksum => Error::Checksum,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
