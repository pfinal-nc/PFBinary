//! PFBinary encoder (streaming + Value convenience).

use pf_format::{
    checksum_bytes, validate_flags, write_base_header, FLAG_CHECKSUM, FLAG_STRDEDUP, TAG_ARRAY_HASH,
    TAG_ARRAY_PACKED, TAG_ARRAY_REPEAT, TAG_ARRAY_ROWS, TAG_DOUBLE, TAG_FALSE, TAG_NULL,
    TAG_PACKED_LONGS, TAG_STR_EMPTY, TAG_STR_REF, TAG_STR_VARINT, TAG_TRUE, TAG_VARINT,
};

use crate::error::{Error, Result};
use crate::string_table::EncodeStringTable;
use crate::value::{Key, Value};
use crate::varint::{write_ivarint, write_uvarint};

pub struct EncodeOptions {
    pub flags: u8,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            flags: FLAG_STRDEDUP,
        }
    }
}

/// Streaming encoder: emit values in document order, then [`Encoder::finish`].
pub struct Encoder {
    buf: Vec<u8>,
    strings: EncodeStringTable,
    strdedup: bool,
    flags: u8,
}

impl Encoder {
    pub fn new(flags: u8) -> Result<Self> {
        validate_flags(flags).map_err(Error::from)?;
        Ok(Self {
            buf: Vec::with_capacity(64),
            strings: EncodeStringTable::new(),
            strdedup: flags & FLAG_STRDEDUP != 0,
            flags,
        })
    }

    pub fn flags(&self) -> u8 {
        self.flags
    }

    pub fn write_null(&mut self) {
        self.buf.push(TAG_NULL);
    }

    pub fn write_bool(&mut self, v: bool) {
        self.buf.push(if v { TAG_TRUE } else { TAG_FALSE });
    }

    pub fn write_i64(&mut self, n: i64) {
        self.buf.push(TAG_VARINT);
        write_ivarint(&mut self.buf, n);
    }

    pub fn write_f64(&mut self, f: f64) {
        self.buf.push(TAG_DOUBLE);
        self.buf.extend_from_slice(&f.to_bits().to_le_bytes());
    }

    pub fn write_string(&mut self, bytes: &[u8]) {
        self.write_string_bytes(bytes);
    }

    pub fn write_array_packed_begin(&mut self, count: u32) {
        self.buf.push(TAG_ARRAY_PACKED);
        write_uvarint(&mut self.buf, count as u64);
    }

    /// Homogeneous packed integer run (no per-element tags).
    pub fn write_packed_longs_begin(&mut self, count: u32) {
        self.buf.push(TAG_PACKED_LONGS);
        write_uvarint(&mut self.buf, count as u64);
    }

    /// Append one zigzag-varint after [`write_packed_longs_begin`] (no type tag).
    pub fn write_packed_long_item(&mut self, n: i64) {
        write_ivarint(&mut self.buf, n);
    }

    pub fn write_array_hash_begin(&mut self, count: u32) {
        self.buf.push(TAG_ARRAY_HASH);
        write_uvarint(&mut self.buf, count as u64);
    }

    /// Homogeneous rowset: shared key schema + row-major values.
    pub fn write_array_rows_begin(&mut self, nrows: u32, ncols: u32) {
        self.buf.push(TAG_ARRAY_ROWS);
        write_uvarint(&mut self.buf, nrows as u64);
        write_uvarint(&mut self.buf, ncols as u64);
    }

    /// Packed list of identical values (write value once).
    pub fn write_array_repeat_begin(&mut self, count: u32) {
        self.buf.push(TAG_ARRAY_REPEAT);
        write_uvarint(&mut self.buf, count as u64);
    }

    /// Integer key for the next HASH entry (must precede the value).
    pub fn write_key_i64(&mut self, n: i64) {
        self.buf.push(TAG_VARINT);
        write_ivarint(&mut self.buf, n);
    }

    /// String key for the next HASH entry (must precede the value).
    pub fn write_key_string(&mut self, bytes: &[u8]) {
        self.write_string_bytes(bytes);
    }

    fn write_string_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            self.buf.push(TAG_STR_EMPTY);
            return;
        }

        if self.strdedup {
            let (id, is_new) = self.strings.intern_get(bytes);
            if !is_new {
                self.buf.push(TAG_STR_REF);
                write_uvarint(&mut self.buf, id as u64);
                return;
            }
        }

        self.buf.push(TAG_STR_VARINT);
        write_uvarint(&mut self.buf, bytes.len() as u64);
        self.buf.extend_from_slice(bytes);
    }

    fn write_value(&mut self, value: &Value) {
        match value {
            Value::Null => self.write_null(),
            Value::Bool(v) => self.write_bool(*v),
            Value::Int(n) => self.write_i64(*n),
            Value::Float(f) => self.write_f64(*f),
            Value::String(s) => self.write_string(s),
            Value::ArrayPacked(items) => {
                if !items.is_empty() && items.iter().all(|x| matches!(x, Value::Int(_))) {
                    self.write_packed_longs_begin(items.len() as u32);
                    for item in items {
                        if let Value::Int(n) = item {
                            self.write_packed_long_item(*n);
                        }
                    }
                } else if items.len() >= 2 && items.iter().all(|x| x == &items[0]) {
                    self.write_array_repeat_begin(items.len() as u32);
                    self.write_value(&items[0]);
                } else if let Some(schema) = rowset_schema(items) {
                    let nrows = items.len() as u32;
                    let ncols = schema.len() as u32;
                    self.write_array_rows_begin(nrows, ncols);
                    for key in &schema {
                        match key {
                            Key::Int(i) => self.write_key_i64(*i),
                            Key::String(s) => self.write_key_string(s),
                        }
                    }
                    for item in items {
                        if let Value::ArrayHash(pairs) = item {
                            for (_, val) in pairs {
                                self.write_value(val);
                            }
                        }
                    }
                } else {
                    self.write_array_packed_begin(items.len() as u32);
                    for item in items {
                        self.write_value(item);
                    }
                }
            }
            Value::ArrayHash(pairs) => {
                self.write_array_hash_begin(pairs.len() as u32);
                for (key, val) in pairs {
                    match key {
                        Key::Int(i) => self.write_key_i64(*i),
                        Key::String(s) => self.write_key_string(s),
                    }
                    self.write_value(val);
                }
            }
        }
    }

    /// Finalize payload and prepend header (+ optional checksum).
    pub fn finish(self) -> Vec<u8> {
        let payload = self.buf;
        let mut out = Vec::with_capacity(6 + payload.len() + 4);
        // flags already validated in new()
        let _ = write_base_header(&mut out, self.flags);
        if self.flags & FLAG_CHECKSUM != 0 {
            out.extend_from_slice(&checksum_bytes(&payload));
        }
        out.extend_from_slice(&payload);
        out
    }
}

/// Encode `value` to a complete PFBinary document (default flags).
pub fn encode(value: &Value) -> Result<Vec<u8>> {
    encode_with(value, &EncodeOptions::default())
}

/// Encode with explicit flags.
pub fn encode_with(value: &Value, opts: &EncodeOptions) -> Result<Vec<u8>> {
    let mut enc = Encoder::new(opts.flags)?;
    enc.write_value(value);
    Ok(enc.finish())
}

/// If `items` is a list of ≥2 hash rows with identical key schemas, return that schema.
fn rowset_schema(items: &[Value]) -> Option<Vec<Key>> {
    if items.len() < 2 {
        return None;
    }
    let schema: Vec<Key> = match &items[0] {
        Value::ArrayHash(pairs)
            if !pairs.is_empty()
                && pairs
                    .iter()
                    .all(|(k, _)| matches!(k, Key::String(_) | Key::Int(_))) =>
        {
            pairs.iter().map(|(k, _)| k.clone()).collect()
        }
        _ => return None,
    };
    for item in &items[1..] {
        match item {
            Value::ArrayHash(pairs) if pairs.len() == schema.len() => {
                for (i, (k, _)) in pairs.iter().enumerate() {
                    if k != &schema[i] {
                        return None;
                    }
                }
            }
            _ => return None,
        }
    }
    Some(schema)
}
