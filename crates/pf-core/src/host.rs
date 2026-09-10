//! Host trait: Decoder builds values through callbacks (no intermediate AST on PHP path).
//!
//! # Production note
//!
//! Prefer [`Host::push_scalar`] / [`Host::insert_scalar`] for array elements so PHP can
//! write stack `zval`s directly into HashTables (no per-scalar `emalloc`).
//!
//! Call [`Host::intern_string`] on first definition so hosts can cache `zend_string`
//! and satisfy `STR_REF` / repeated keys via [`Scalar::StringId`] / [`ArrayKey::StringId`].
//!
//! # Drop / error semantics
//!
//! If decoding fails after `begin_array`, the `ArrayState` is dropped without
//! calling `end_array`. Implementations **must** free any Zend/PHP resources in
//! `Drop` for both `Value` and `ArrayState`.

use crate::error::Result;

/// Key kinds for associative arrays.
#[derive(Debug, Clone, Copy)]
pub enum ArrayKey<'a> {
    Int(i64),
    String(&'a [u8]),
    /// Previously [`Host::intern_string`]-registered id (STR_REF / hashed keys).
    StringId(u32),
}

/// Scalar payload for direct array writes (production hot path).
#[derive(Debug, Clone, Copy)]
pub enum Scalar<'a> {
    Null,
    Bool(bool),
    I64(i64),
    F64(f64),
    String(&'a [u8]),
    /// Previously interned string id.
    StringId(u32),
}

/// Callback surface implemented by PHP adapter or test harness.
pub trait Host {
    type Value;
    type ArrayState;

    fn make_null(&mut self) -> Result<Self::Value>;
    fn make_bool(&mut self, v: bool) -> Result<Self::Value>;
    fn make_i64(&mut self, v: i64) -> Result<Self::Value>;
    fn make_f64(&mut self, v: f64) -> Result<Self::Value>;
    fn make_string(&mut self, bytes: &[u8]) -> Result<Self::Value>;

    /// Materialize a string from an id registered via [`Host::intern_string`].
    fn make_string_id(&mut self, id: u32) -> Result<Self::Value>;

    /// Register string table entry `id` (must be dense from 0). Default: no-op.
    fn intern_string(&mut self, id: u32, bytes: &[u8]) -> Result<()> {
        let _ = (id, bytes);
        Ok(())
    }

    fn begin_array(&mut self, packed: bool, count: u32) -> Result<Self::ArrayState>;
    fn push_packed(&mut self, arr: &mut Self::ArrayState, val: Self::Value) -> Result<()>;
    fn insert_key(
        &mut self,
        arr: &mut Self::ArrayState,
        key: ArrayKey<'_>,
        val: Self::Value,
    ) -> Result<()>;
    fn end_array(&mut self, arr: Self::ArrayState) -> Result<Self::Value>;

    /// Deep-ish duplicate of a finished value (for `ARRAY_REPEAT`).
    fn duplicate_value(&mut self, val: &Self::Value) -> Result<Self::Value>;

    /// Direct packed push of a scalar (default: make + push_packed).
    fn push_scalar(&mut self, arr: &mut Self::ArrayState, scalar: Scalar<'_>) -> Result<()> {
        let v = match scalar {
            Scalar::Null => self.make_null()?,
            Scalar::Bool(b) => self.make_bool(b)?,
            Scalar::I64(i) => self.make_i64(i)?,
            Scalar::F64(f) => self.make_f64(f)?,
            Scalar::String(s) => self.make_string(s)?,
            Scalar::StringId(id) => self.make_string_id(id)?,
        };
        self.push_packed(arr, v)
    }

    /// Direct hash insert of a scalar (default: make + insert_key).
    fn insert_scalar(
        &mut self,
        arr: &mut Self::ArrayState,
        key: ArrayKey<'_>,
        scalar: Scalar<'_>,
    ) -> Result<()> {
        let v = match scalar {
            Scalar::Null => self.make_null()?,
            Scalar::Bool(b) => self.make_bool(b)?,
            Scalar::I64(i) => self.make_i64(i)?,
            Scalar::F64(f) => self.make_f64(f)?,
            Scalar::String(s) => self.make_string(s)?,
            Scalar::StringId(id) => self.make_string_id(id)?,
        };
        self.insert_key(arr, key, v)
    }

    /// Bulk append packed integers (default: loop `push_scalar`).
    fn push_i64_run(&mut self, arr: &mut Self::ArrayState, values: &[i64]) -> Result<()> {
        for &v in values {
            self.push_scalar(arr, Scalar::I64(v))?;
        }
        Ok(())
    }
}
