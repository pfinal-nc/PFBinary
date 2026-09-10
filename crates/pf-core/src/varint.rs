//! LEB128 uvarint + ZigZag i64 (protocol §6).

use crate::error::{Error, Result};

const MAX_VARINT_LEN: usize = 10;

#[inline]
pub fn zigzag_encode(n: i64) -> u64 {
    ((n << 1) ^ (n >> 63)) as u64
}

#[inline]
pub fn zigzag_decode(u: u64) -> i64 {
    ((u >> 1) as i64) ^ (-((u & 1) as i64))
}

/// Encode unsigned LEB128 into `out`.
pub fn write_uvarint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
            out.push(byte);
        } else {
            out.push(byte);
            break;
        }
    }
}

/// Encode signed i64 as ZigZag + LEB128.
#[inline]
pub fn write_ivarint(out: &mut Vec<u8>, value: i64) {
    write_uvarint(out, zigzag_encode(value));
}

/// Read unsigned LEB128; returns `(value, bytes_consumed)`.
pub fn read_uvarint(input: &[u8]) -> Result<(u64, usize)> {
    let mut result: u64 = 0;
    let mut shift = 0u32;
    for (i, &byte) in input.iter().enumerate() {
        if i >= MAX_VARINT_LEN {
            return Err(Error::VarInt);
        }
        let bits = (byte & 0x7f) as u64;
        // Byte index 9 (0-based): only the low bit may be set (u64 has 64 bits; 9*7=63).
        if i == MAX_VARINT_LEN - 1 {
            if bits > 1 || byte & 0x80 != 0 {
                return Err(Error::VarInt);
            }
            result |= bits << 63;
            return Ok((result, i + 1));
        }
        result |= bits << shift;
        if byte & 0x80 == 0 {
            return Ok((result, i + 1));
        }
        shift += 7;
    }
    Err(Error::Truncated)
}

/// Read ZigZag-decoded i64; returns `(value, bytes_consumed)`.
#[inline]
pub fn read_ivarint(input: &[u8]) -> Result<(i64, usize)> {
    let (u, n) = read_uvarint(input)?;
    Ok((zigzag_decode(u), n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zigzag_vectors() {
        let cases: &[(i64, u64)] = &[
            (0, 0),
            (-1, 1),
            (1, 2),
            (63, 126),
            (-64, 127),
            (64, 128),
            (10001, 20002),
        ];
        for &(n, z) in cases {
            assert_eq!(zigzag_encode(n), z, "encode {n}");
            assert_eq!(zigzag_decode(z), n, "decode {z}");
        }
    }

    #[test]
    fn leb128_10001() {
        let mut buf = Vec::new();
        write_ivarint(&mut buf, 10001);
        assert_eq!(buf, vec![0xA2, 0x9C, 0x01]);
        let (v, n) = read_ivarint(&buf).unwrap();
        assert_eq!(v, 10001);
        assert_eq!(n, 3);
    }

    #[test]
    fn roundtrip_edges() {
        for n in [i64::MIN, i64::MAX, -2, 127, -128, 255] {
            let mut buf = Vec::new();
            write_ivarint(&mut buf, n);
            let (v, _) = read_ivarint(&buf).unwrap();
            assert_eq!(v, n);
        }
    }

    #[test]
    fn reject_overlong_10th_byte() {
        let bad = [0x80u8, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x02];
        assert_eq!(read_uvarint(&bad).unwrap_err(), Error::VarInt);
    }
}
