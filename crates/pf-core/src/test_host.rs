//! Test Host: builds [`crate::value::Value`] via Host callbacks.

use crate::error::{Error, Result};
use crate::host::{ArrayKey, Host};
use crate::value::{Key, Value};

#[derive(Debug, Default)]
pub struct TestHost {
    strings: Vec<Vec<u8>>,
}

pub struct TestArray {
    packed: bool,
    items: Vec<(Key, Value)>,
}

impl Host for TestHost {
    type Value = Value;
    type ArrayState = TestArray;

    fn make_null(&mut self) -> Result<Self::Value> {
        Ok(Value::Null)
    }

    fn make_bool(&mut self, v: bool) -> Result<Self::Value> {
        Ok(Value::Bool(v))
    }

    fn make_i64(&mut self, v: i64) -> Result<Self::Value> {
        Ok(Value::Int(v))
    }

    fn make_f64(&mut self, v: f64) -> Result<Self::Value> {
        Ok(Value::Float(v))
    }

    fn make_string(&mut self, bytes: &[u8]) -> Result<Self::Value> {
        Ok(Value::String(bytes.to_vec()))
    }

    fn make_string_id(&mut self, id: u32) -> Result<Self::Value> {
        let bytes = self
            .strings
            .get(id as usize)
            .ok_or(Error::BadStrRef)?;
        Ok(Value::String(bytes.clone()))
    }

    fn intern_string(&mut self, id: u32, bytes: &[u8]) -> Result<()> {
        if id as usize != self.strings.len() {
            return Err(Error::Internal);
        }
        self.strings.push(bytes.to_vec());
        Ok(())
    }

    fn begin_array(&mut self, packed: bool, count: u32) -> Result<Self::ArrayState> {
        Ok(TestArray {
            packed,
            items: Vec::with_capacity(count as usize),
        })
    }

    fn push_packed(&mut self, arr: &mut Self::ArrayState, val: Self::Value) -> Result<()> {
        let idx = arr.items.len() as i64;
        arr.items.push((Key::Int(idx), val));
        Ok(())
    }

    fn insert_key(
        &mut self,
        arr: &mut Self::ArrayState,
        key: ArrayKey<'_>,
        val: Self::Value,
    ) -> Result<()> {
        let k = match key {
            ArrayKey::Int(i) => Key::Int(i),
            ArrayKey::String(s) => Key::String(s.to_vec()),
            ArrayKey::StringId(id) => {
                let bytes = self
                    .strings
                    .get(id as usize)
                    .ok_or(Error::BadStrRef)?;
                Key::String(bytes.clone())
            }
        };
        arr.items.push((k, val));
        Ok(())
    }

    fn end_array(&mut self, arr: Self::ArrayState) -> Result<Self::Value> {
        if arr.packed {
            Ok(Value::ArrayPacked(
                arr.items.into_iter().map(|(_, v)| v).collect(),
            ))
        } else {
            Ok(Value::ArrayHash(arr.items))
        }
    }

    fn duplicate_value(&mut self, val: &Self::Value) -> Result<Self::Value> {
        Ok(val.clone())
    }
}
