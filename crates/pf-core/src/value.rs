//! In-memory value tree used for encode input and TestHost decode output.

use std::collections::BTreeMap;

/// Array key in the test/value model.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    Int(i64),
    String(Vec<u8>),
}

impl Key {
    pub fn str(s: impl AsRef<[u8]>) -> Self {
        Key::String(s.as_ref().to_vec())
    }
}

/// Logical value for roundtrip tests and encode API.
#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(Vec<u8>),
    /// Packed PHP array (keys 0..n-1).
    ArrayPacked(Vec<Value>),
    /// Associative / mixed array.
    ArrayHash(Vec<(Key, Value)>),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a.to_bits() == b.to_bits(),
            (Value::String(a), Value::String(b)) => a == b,
            (Value::ArrayPacked(a), Value::ArrayPacked(b)) => a == b,
            (Value::ArrayHash(a), Value::ArrayHash(b)) => a == b,
            // Treat empty packed/hash as equal
            (Value::ArrayPacked(a), Value::ArrayHash(b)) if a.is_empty() && b.is_empty() => true,
            (Value::ArrayHash(a), Value::ArrayPacked(b)) if a.is_empty() && b.is_empty() => true,
            _ => false,
        }
    }
}

impl Eq for Value {}

impl Value {
    pub fn str(s: impl AsRef<[u8]>) -> Self {
        Value::String(s.as_ref().to_vec())
    }

    /// Build a hash array from string keys (UTF-8 / bytes).
    pub fn map_str(pairs: Vec<(&str, Value)>) -> Self {
        Value::ArrayHash(
            pairs
                .into_iter()
                .map(|(k, v)| (Key::str(k), v))
                .collect(),
        )
    }
}

/// Optional: convert hash with contiguous int keys into packed (encode helper).
pub fn maybe_pack(hash: Vec<(Key, Value)>) -> Value {
    let n = hash.len();
    let packed = (0..n).all(|i| {
        hash.get(i)
            .map(|(k, _)| matches!(k, Key::Int(x) if *x == i as i64))
            .unwrap_or(false)
    });
    if packed {
        Value::ArrayPacked(hash.into_iter().map(|(_, v)| v).collect())
    } else {
        Value::ArrayHash(hash)
    }
}

/// Stable debug helper for map equality checks ignoring packed vs hash for same content.
pub fn normalize_for_cmp(v: &Value) -> Value {
    match v {
        Value::ArrayPacked(items) => Value::ArrayHash(
            items
                .iter()
                .enumerate()
                .map(|(i, x)| (Key::Int(i as i64), normalize_for_cmp(x)))
                .collect(),
        ),
        Value::ArrayHash(items) => {
            let mut m: BTreeMap<Key, Value> = BTreeMap::new();
            for (k, val) in items {
                m.insert(k.clone(), normalize_for_cmp(val));
            }
            Value::ArrayHash(m.into_iter().collect())
        }
        Value::String(s) => Value::String(s.clone()),
        Value::Null => Value::Null,
        Value::Bool(b) => Value::Bool(*b),
        Value::Int(i) => Value::Int(*i),
        Value::Float(f) => Value::Float(*f),
    }
}
