//! PFBinary decoder: bytes → Host callbacks (no intermediate AST).

use pf_format::{
    is_v1_key_tag, parse_header, FLAG_STRDEDUP, TAG_ARRAY_HASH, TAG_ARRAY_PACKED, TAG_ARRAY_REPEAT,
    TAG_ARRAY_ROWS, TAG_DOUBLE, TAG_FALSE, TAG_NULL, TAG_PACKED_LONGS, TAG_STR_EMPTY, TAG_STR_REF,
    TAG_STR_VARINT, TAG_TRUE, TAG_VARINT,
};

use crate::error::{Error, Result};
use crate::host::{ArrayKey, Host, Scalar};
use crate::string_table::DecodeStringTable;
use crate::varint::{read_ivarint, read_uvarint};

pub const DEFAULT_MAX_DEPTH: u32 = 100;
pub const DEFAULT_MAX_SIZE: usize = 10 * 1024 * 1024;

pub struct DecodeOptions {
    pub max_depth: u32,
    pub max_size: usize,
    /// Reject trailing bytes after the top-level value (default true).
    pub strict_trailing: bool,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self {
            max_depth: DEFAULT_MAX_DEPTH,
            max_size: DEFAULT_MAX_SIZE,
            strict_trailing: true,
        }
    }
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.remaining() < n {
            return Err(Error::Truncated);
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn rest(&self) -> &'a [u8] {
        &self.data[self.pos..]
    }

    fn advance(&mut self, n: usize) {
        self.pos += n;
    }
}

enum ArrayKeyOwned {
    Int(i64),
    Empty,
    StringId(u32),
}

struct Decoder<'a, H: Host> {
    cur: Cursor<'a>,
    host: &'a mut H,
    strings: DecodeStringTable,
    depth: u32,
    max_depth: u32,
    max_size: usize,
    size_used: usize,
    /// When false, `STR_REF` is rejected (Flags bit1 clear).
    strdedup: bool,
}

impl<'a, H: Host> Decoder<'a, H> {
    fn charge(&mut self, n: usize) -> Result<()> {
        self.size_used = self.size_used.checked_add(n).ok_or(Error::Overflow)?;
        if self.size_used > self.max_size {
            return Err(Error::MaxSize);
        }
        Ok(())
    }

    fn read_uvarint(&mut self) -> Result<u64> {
        let (v, n) = read_uvarint(self.cur.rest())?;
        self.cur.advance(n);
        Ok(v)
    }

    fn read_ivarint(&mut self) -> Result<i64> {
        let (v, n) = read_ivarint(self.cur.rest())?;
        self.cur.advance(n);
        Ok(v)
    }

    /// First definition of a non-empty string: intern on host + local table.
    fn define_string(&mut self, bytes: &'a [u8]) -> Result<u32> {
        let id = self.strings.len() as u32;
        self.host.intern_string(id, bytes)?;
        self.strings.push(bytes.to_vec());
        Ok(id)
    }

    fn decode_string_tag(&mut self, tag: u8) -> Result<H::Value> {
        match tag {
            TAG_STR_EMPTY => self.host.make_string(b""),
            TAG_STR_VARINT => {
                let len = self.read_uvarint()?;
                if len > self.cur.remaining() as u64 {
                    return Err(Error::Overflow);
                }
                let len = len as usize;
                self.charge(len)?;
                let bytes = self.cur.take(len)?;
                let id = self.define_string(bytes)?;
                self.host.make_string_id(id)
            }
            TAG_STR_REF => {
                if !self.strdedup {
                    return Err(Error::UnknownTag);
                }
                let id = self.read_uvarint()?;
                if id > u32::MAX as u64 {
                    return Err(Error::BadStrRef);
                }
                let id = id as u32;
                let len = self.strings.get(id).ok_or(Error::BadStrRef)?.len();
                // Charge every materialization: STR_REF can amplify Host memory.
                self.charge(len)?;
                self.host.make_string_id(id)
            }
            _ => Err(Error::UnknownTag),
        }
    }

    fn read_key_owned(&mut self) -> Result<ArrayKeyOwned> {
        let tag = self.cur.u8()?;
        if !is_v1_key_tag(tag) {
            return Err(Error::BadKey);
        }
        match tag {
            TAG_VARINT => Ok(ArrayKeyOwned::Int(self.read_ivarint()?)),
            TAG_STR_EMPTY => Ok(ArrayKeyOwned::Empty),
            TAG_STR_VARINT => {
                let len = self.read_uvarint()?;
                if len > self.cur.remaining() as u64 {
                    return Err(Error::Overflow);
                }
                let len = len as usize;
                self.charge(len)?;
                let bytes = self.cur.take(len)?;
                let id = self.define_string(bytes)?;
                Ok(ArrayKeyOwned::StringId(id))
            }
            TAG_STR_REF => {
                if !self.strdedup {
                    return Err(Error::BadKey);
                }
                let id = self.read_uvarint()?;
                if id > u32::MAX as u64 {
                    return Err(Error::BadStrRef);
                }
                let id = id as u32;
                let len = self.strings.get(id).ok_or(Error::BadStrRef)?.len();
                self.charge(len)?;
                Ok(ArrayKeyOwned::StringId(id))
            }
            _ => Err(Error::BadKey),
        }
    }

    /// Reject absurd counts before `Host::begin_array` allocates.
    ///
    /// `count` is bounded by remaining input bytes (and payload ≤ max_size),
    /// so Host pre-allocation is O(input size).
    fn check_array_count(&self, count64: u64, min_bytes_per_elem: u64) -> Result<u32> {
        if count64 > u32::MAX as u64 {
            return Err(Error::Overflow);
        }
        let need = count64.saturating_mul(min_bytes_per_elem);
        if need > self.cur.remaining() as u64 {
            return Err(Error::Overflow);
        }
        Ok(count64 as u32)
    }

    fn decode_value(&mut self) -> Result<H::Value> {
        let tag = self.cur.u8()?;
        match tag {
            TAG_NULL => self.host.make_null(),
            TAG_FALSE => self.host.make_bool(false),
            TAG_TRUE => self.host.make_bool(true),
            TAG_VARINT => {
                let n = self.read_ivarint()?;
                self.host.make_i64(n)
            }
            TAG_DOUBLE => {
                let raw = self.cur.take(8)?;
                let mut le = [0u8; 8];
                le.copy_from_slice(raw);
                let f = f64::from_bits(u64::from_le_bytes(le));
                self.host.make_f64(f)
            }
            TAG_STR_EMPTY | TAG_STR_VARINT | TAG_STR_REF => self.decode_string_tag(tag),
            TAG_ARRAY_PACKED => self.decode_array_packed(),
            TAG_ARRAY_HASH => self.decode_array_hash(),
            TAG_PACKED_LONGS => self.decode_packed_longs(),
            TAG_ARRAY_ROWS => self.decode_array_rows(),
            TAG_ARRAY_REPEAT => self.decode_array_repeat(),
            _ => Err(Error::UnknownTag),
        }
    }

    fn enter_depth(&mut self) -> Result<()> {
        self.depth = self.depth.checked_add(1).ok_or(Error::MaxDepth)?;
        if self.depth > self.max_depth {
            return Err(Error::MaxDepth);
        }
        Ok(())
    }

    fn leave_depth(&mut self) {
        self.depth -= 1;
    }

    fn decode_array_packed(&mut self) -> Result<H::Value> {
        let count64 = self.read_uvarint()?;
        let count = self.check_array_count(count64, 1)?;
        self.enter_depth()?;
        let mut arr = self.host.begin_array(true, count)?;
        for _ in 0..count {
            self.decode_into_packed(&mut arr)?;
        }
        self.leave_depth();
        self.host.end_array(arr)
    }

    fn decode_array_hash(&mut self) -> Result<H::Value> {
        let count64 = self.read_uvarint()?;
        let count = self.check_array_count(count64, 2)?;
        self.enter_depth()?;
        let mut arr = self.host.begin_array(false, count)?;
        for _ in 0..count {
            let key = self.read_key_owned()?;
            self.decode_into_hash(&mut arr, &key)?;
        }
        self.leave_depth();
        self.host.end_array(arr)
    }

    /// Decode one array element into a packed array (scalars via direct write).
    fn decode_into_packed(&mut self, arr: &mut H::ArrayState) -> Result<()> {
        let tag = self.cur.u8()?;
        match tag {
            TAG_NULL => self.host.push_scalar(arr, Scalar::Null),
            TAG_FALSE => self.host.push_scalar(arr, Scalar::Bool(false)),
            TAG_TRUE => self.host.push_scalar(arr, Scalar::Bool(true)),
            TAG_VARINT => {
                let n = self.read_ivarint()?;
                self.host.push_scalar(arr, Scalar::I64(n))
            }
            TAG_DOUBLE => {
                let f = self.read_f64()?;
                self.host.push_scalar(arr, Scalar::F64(f))
            }
            TAG_STR_EMPTY => self.host.push_scalar(arr, Scalar::String(b"")),
            TAG_STR_VARINT => {
                let id = self.read_str_varint_define()?;
                self.host.push_scalar(arr, Scalar::StringId(id))
            }
            TAG_STR_REF => {
                let id = self.read_str_ref_id()?;
                self.host.push_scalar(arr, Scalar::StringId(id))
            }
            TAG_ARRAY_PACKED => {
                let v = self.decode_array_packed()?;
                self.host.push_packed(arr, v)
            }
            TAG_ARRAY_HASH => {
                let v = self.decode_array_hash()?;
                self.host.push_packed(arr, v)
            }
            TAG_PACKED_LONGS => {
                let v = self.decode_packed_longs()?;
                self.host.push_packed(arr, v)
            }
            TAG_ARRAY_ROWS => {
                let v = self.decode_array_rows()?;
                self.host.push_packed(arr, v)
            }
            TAG_ARRAY_REPEAT => {
                let v = self.decode_array_repeat()?;
                self.host.push_packed(arr, v)
            }
            _ => Err(Error::UnknownTag),
        }
    }

    fn decode_into_hash(&mut self, arr: &mut H::ArrayState, key: &ArrayKeyOwned) -> Result<()> {
        let key_ref = match key {
            ArrayKeyOwned::Int(i) => ArrayKey::Int(*i),
            ArrayKeyOwned::Empty => ArrayKey::String(b""),
            ArrayKeyOwned::StringId(id) => ArrayKey::StringId(*id),
        };
        let tag = self.cur.u8()?;
        match tag {
            TAG_NULL => self.host.insert_scalar(arr, key_ref, Scalar::Null),
            TAG_FALSE => self.host.insert_scalar(arr, key_ref, Scalar::Bool(false)),
            TAG_TRUE => self.host.insert_scalar(arr, key_ref, Scalar::Bool(true)),
            TAG_VARINT => {
                let n = self.read_ivarint()?;
                self.host.insert_scalar(arr, key_ref, Scalar::I64(n))
            }
            TAG_DOUBLE => {
                let f = self.read_f64()?;
                self.host.insert_scalar(arr, key_ref, Scalar::F64(f))
            }
            TAG_STR_EMPTY => self.host.insert_scalar(arr, key_ref, Scalar::String(b"")),
            TAG_STR_VARINT => {
                let id = self.read_str_varint_define()?;
                self.host.insert_scalar(arr, key_ref, Scalar::StringId(id))
            }
            TAG_STR_REF => {
                let id = self.read_str_ref_id()?;
                self.host.insert_scalar(arr, key_ref, Scalar::StringId(id))
            }
            TAG_ARRAY_PACKED => {
                let v = self.decode_array_packed()?;
                self.host.insert_key(arr, key_ref, v)
            }
            TAG_ARRAY_HASH => {
                let v = self.decode_array_hash()?;
                self.host.insert_key(arr, key_ref, v)
            }
            TAG_PACKED_LONGS => {
                let v = self.decode_packed_longs()?;
                self.host.insert_key(arr, key_ref, v)
            }
            TAG_ARRAY_ROWS => {
                let v = self.decode_array_rows()?;
                self.host.insert_key(arr, key_ref, v)
            }
            TAG_ARRAY_REPEAT => {
                let v = self.decode_array_repeat()?;
                self.host.insert_key(arr, key_ref, v)
            }
            _ => Err(Error::UnknownTag),
        }
    }

    fn decode_array_repeat(&mut self) -> Result<H::Value> {
        let count64 = self.read_uvarint()?;
        if count64 > u32::MAX as u64 {
            return Err(Error::Overflow);
        }
        let count = count64 as u32;
        // Payload is a single Value, not `count` values.
        if self.cur.remaining() < 1 && count > 0 {
            return Err(Error::Truncated);
        }
        self.enter_depth()?;
        let template = self.decode_value()?;
        let mut arr = self.host.begin_array(true, count)?;
        for _ in 0..count {
            let item = self.host.duplicate_value(&template)?;
            self.host.push_packed(&mut arr, item)?;
        }
        drop(template);
        self.leave_depth();
        self.host.end_array(arr)
    }

    fn decode_array_rows(&mut self) -> Result<H::Value> {
        let nrows64 = self.read_uvarint()?;
        let ncols64 = self.read_uvarint()?;
        if nrows64 > u32::MAX as u64 || ncols64 > u32::MAX as u64 {
            return Err(Error::Overflow);
        }
        let nrows = nrows64 as u32;
        let ncols = ncols64 as u32;
        // Schema (≥1 byte/key) + values (≥1 byte/cell when ncols>0).
        let need = ncols64.saturating_add(nrows64.saturating_mul(ncols64.max(1)));
        if need > self.cur.remaining() as u64 {
            return Err(Error::Overflow);
        }
        // Also bound outer array by remaining (same spirit as check_array_count).
        if nrows64 > 0 && (nrows64 as u64) > self.cur.remaining() as u64 {
            return Err(Error::Overflow);
        }

        self.enter_depth()?;
        let mut keys = Vec::with_capacity(ncols as usize);
        for _ in 0..ncols {
            keys.push(self.read_key_owned()?);
        }

        let mut outer = self.host.begin_array(true, nrows)?;
        for _ in 0..nrows {
            self.enter_depth()?;
            let mut row = self.host.begin_array(false, ncols)?;
            for key in &keys {
                self.decode_into_hash(&mut row, key)?;
            }
            self.leave_depth();
            let row_val = self.host.end_array(row)?;
            self.host.push_packed(&mut outer, row_val)?;
        }
        self.leave_depth();
        self.host.end_array(outer)
    }

    fn decode_packed_longs(&mut self) -> Result<H::Value> {
        let count64 = self.read_uvarint()?;
        // Each long is at least 1 byte.
        let count = self.check_array_count(count64, 1)?;
        self.enter_depth()?;
        let mut arr = self.host.begin_array(true, count)?;
        // Read into a small stack buffer or heap once, then one bulk host call.
        let mut vals = Vec::with_capacity(count as usize);
        for _ in 0..count {
            vals.push(self.read_ivarint()?);
        }
        self.host.push_i64_run(&mut arr, &vals)?;
        self.leave_depth();
        self.host.end_array(arr)
    }

    fn read_f64(&mut self) -> Result<f64> {
        let raw = self.cur.take(8)?;
        let mut le = [0u8; 8];
        le.copy_from_slice(raw);
        Ok(f64::from_bits(u64::from_le_bytes(le)))
    }

    fn read_str_varint_define(&mut self) -> Result<u32> {
        let len = self.read_uvarint()?;
        if len > self.cur.remaining() as u64 {
            return Err(Error::Overflow);
        }
        let len = len as usize;
        self.charge(len)?;
        let bytes = self.cur.take(len)?;
        self.define_string(bytes)
    }

    fn read_str_ref_id(&mut self) -> Result<u32> {
        if !self.strdedup {
            return Err(Error::UnknownTag);
        }
        let id = self.read_uvarint()?;
        if id > u32::MAX as u64 {
            return Err(Error::BadStrRef);
        }
        let id = id as u32;
        let len = self.strings.get(id).ok_or(Error::BadStrRef)?.len();
        self.charge(len)?;
        Ok(id)
    }
}

/// Decode a complete PFBinary document into a Host value.
pub fn decode<H: Host>(data: &[u8], host: &mut H) -> Result<H::Value> {
    decode_with(data, host, &DecodeOptions::default())
}

pub fn decode_with<H: Host>(
    data: &[u8],
    host: &mut H,
    opts: &DecodeOptions,
) -> Result<H::Value> {
    let (header, payload) = parse_header(data).map_err(Error::from)?;
    let max_size = if opts.max_size == 0 {
        DEFAULT_MAX_SIZE
    } else {
        opts.max_size
    };
    // Bound by declared max_size before any Host allocation.
    if payload.len() > max_size {
        return Err(Error::MaxSize);
    }

    let mut dec = Decoder {
        cur: Cursor::new(payload),
        host,
        strings: DecodeStringTable::new(),
        depth: 0,
        max_depth: if opts.max_depth == 0 {
            DEFAULT_MAX_DEPTH
        } else {
            opts.max_depth
        },
        max_size,
        size_used: 0,
        strdedup: header.flags & FLAG_STRDEDUP != 0,
    };

    let value = dec.decode_value()?;
    if opts.strict_trailing && dec.cur.remaining() != 0 {
        return Err(Error::Trailing);
    }
    Ok(value)
}
