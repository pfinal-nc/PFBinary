//! PFBinary document header: Magic / Version / Flags / optional checksum.

use xxhash_rust::xxh3::xxh3_64;

use crate::version::VERSION_V1;

/// ASCII magic `PFBN`.
pub const MAGIC: [u8; 4] = *b"PFBN";

/// Flags bit0: payload checksum present (XXH3 low 32-bit, LE).
pub const FLAG_CHECKSUM: u8 = 1 << 0;
/// Flags bit1: string deduplication enabled (`STR_REF` allowed).
pub const FLAG_STRDEDUP: u8 = 1 << 1;

/// Bits that must be zero in V1.
pub const FLAGS_RESERVED_MASK: u8 = 0xFC;

/// Recommended default flags: string dedup on, checksum off.
pub const FLAGS_DEFAULT: u8 = FLAG_STRDEDUP;

/// Fixed header size without checksum (magic + version + flags).
pub const HEADER_BASE_LEN: usize = 6;
/// Checksum field length when enabled.
pub const CHECKSUM_LEN: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderError {
    Truncated,
    BadMagic,
    UnsupportedVersion,
    UnknownFlags,
    Checksum,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub version: u8,
    pub flags: u8,
}

impl Header {
    #[inline]
    pub fn checksum_enabled(self) -> bool {
        self.flags & FLAG_CHECKSUM != 0
    }

    #[inline]
    pub fn strdedup_enabled(self) -> bool {
        self.flags & FLAG_STRDEDUP != 0
    }

    #[inline]
    pub fn header_len(self) -> usize {
        if self.checksum_enabled() {
            HEADER_BASE_LEN + CHECKSUM_LEN
        } else {
            HEADER_BASE_LEN
        }
    }
}

/// Validate flags: reserved bits must be clear.
#[inline]
pub fn validate_flags(flags: u8) -> Result<(), HeaderError> {
    if flags & FLAGS_RESERVED_MASK != 0 {
        Err(HeaderError::UnknownFlags)
    } else {
        Ok(())
    }
}

/// XXH3-64 low 32 bits of `payload`, as little-endian bytes.
#[inline]
pub fn checksum_bytes(payload: &[u8]) -> [u8; 4] {
    let h = xxh3_64(payload) as u32;
    h.to_le_bytes()
}

/// Write base header (no checksum bytes) into `out`.
pub fn write_base_header(out: &mut Vec<u8>, flags: u8) -> Result<(), HeaderError> {
    validate_flags(flags)?;
    out.extend_from_slice(&MAGIC);
    out.push(VERSION_V1);
    out.push(flags);
    Ok(())
}

/// Parse header and return `(header, payload_slice)`.
///
/// When checksum is enabled, verifies it against the payload before returning.
pub fn parse_header(data: &[u8]) -> Result<(Header, &[u8]), HeaderError> {
    if data.len() < HEADER_BASE_LEN {
        return Err(HeaderError::Truncated);
    }
    if data[0..4] != MAGIC {
        return Err(HeaderError::BadMagic);
    }
    let version = data[4];
    if version != VERSION_V1 {
        return Err(HeaderError::UnsupportedVersion);
    }
    let flags = data[5];
    validate_flags(flags)?;

    let header = Header { version, flags };
    if header.checksum_enabled() {
        if data.len() < HEADER_BASE_LEN + CHECKSUM_LEN {
            return Err(HeaderError::Truncated);
        }
        let stored = &data[HEADER_BASE_LEN..HEADER_BASE_LEN + CHECKSUM_LEN];
        let payload = &data[HEADER_BASE_LEN + CHECKSUM_LEN..];
        let expected = checksum_bytes(payload);
        if stored != expected {
            return Err(HeaderError::Checksum);
        }
        Ok((header, payload))
    } else {
        Ok((header, &data[HEADER_BASE_LEN..]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_default_header() {
        let mut buf = Vec::new();
        write_base_header(&mut buf, FLAGS_DEFAULT).unwrap();
        buf.extend_from_slice(&[0x00]); // NULL payload
        let (h, payload) = parse_header(&buf).unwrap();
        assert!(h.strdedup_enabled());
        assert!(!h.checksum_enabled());
        assert_eq!(payload, &[0x00]);
    }

    #[test]
    fn checksum_roundtrip() {
        let payload = [0x08, 0x04, 0x03, 0x02, 0x03, 0x04, 0x03, 0x06, 0x03, 0x08];
        let mut buf = Vec::new();
        write_base_header(&mut buf, FLAG_CHECKSUM | FLAG_STRDEDUP).unwrap();
        let cs = checksum_bytes(&payload);
        buf.extend_from_slice(&cs);
        buf.extend_from_slice(&payload);
        let (h, p) = parse_header(&buf).unwrap();
        assert!(h.checksum_enabled());
        assert_eq!(p, &payload);
    }

    #[test]
    fn bad_magic() {
        let data = b"XXXX\x01\x02\x00";
        assert_eq!(parse_header(data), Err(HeaderError::BadMagic));
    }
}
