//! Inline string table (encode + decode sides).

use std::collections::HashMap;

/// Encoder-side table: first occurrence assigns ascending ids from 0.
#[derive(Debug, Default)]
pub struct EncodeStringTable {
    map: HashMap<Vec<u8>, u32>,
}

impl EncodeStringTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up or insert. Returns `(id, is_new)`.
    pub fn intern_get(&mut self, bytes: &[u8]) -> (u32, bool) {
        if let Some(&id) = self.map.get(bytes) {
            return (id, false);
        }
        let id = self.map.len() as u32;
        self.map.insert(bytes.to_vec(), id);
        (id, true)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// Decoder-side table: stores raw bytes for STR_REF lookup.
#[derive(Debug, Default)]
pub struct DecodeStringTable {
    entries: Vec<Vec<u8>>,
}

impl DecodeStringTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, bytes: Vec<u8>) -> u32 {
        let id = self.entries.len() as u32;
        self.entries.push(bytes);
        id
    }

    pub fn get(&self, id: u32) -> Option<&[u8]> {
        self.entries.get(id as usize).map(|v| v.as_slice())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}
