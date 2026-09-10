//! C host vtable adapter implementing [`pf_core::Host`].

use std::os::raw::c_void;
use std::ptr;

use pf_core::error::{Error, Result};
use pf_core::host::{ArrayKey, Host, Scalar};

pub type HostFnResult = i32;

pub const PF_SCALAR_NULL: u8 = 0;
pub const PF_SCALAR_BOOL: u8 = 1;
pub const PF_SCALAR_I64: u8 = 2;
pub const PF_SCALAR_F64: u8 = 3;
pub const PF_SCALAR_STRING: u8 = 4;
pub const PF_SCALAR_STRING_ID: u8 = 5;

#[repr(C)]
pub struct PfScalar {
    pub kind: u8,
    pub bool_val: i32,
    pub i64_val: i64,
    pub f64_val: f64,
    pub str: *const u8,
    pub str_len: usize,
    pub str_id: u32,
}

impl PfScalar {
    fn from_scalar(s: Scalar<'_>) -> Self {
        match s {
            Scalar::Null => Self {
                kind: PF_SCALAR_NULL,
                bool_val: 0,
                i64_val: 0,
                f64_val: 0.0,
                str: ptr::null(),
                str_len: 0,
                str_id: 0,
            },
            Scalar::Bool(b) => Self {
                kind: PF_SCALAR_BOOL,
                bool_val: b as i32,
                i64_val: 0,
                f64_val: 0.0,
                str: ptr::null(),
                str_len: 0,
                str_id: 0,
            },
            Scalar::I64(i) => Self {
                kind: PF_SCALAR_I64,
                bool_val: 0,
                i64_val: i,
                f64_val: 0.0,
                str: ptr::null(),
                str_len: 0,
                str_id: 0,
            },
            Scalar::F64(f) => Self {
                kind: PF_SCALAR_F64,
                bool_val: 0,
                i64_val: 0,
                f64_val: f,
                str: ptr::null(),
                str_len: 0,
                str_id: 0,
            },
            Scalar::String(bytes) => Self {
                kind: PF_SCALAR_STRING,
                bool_val: 0,
                i64_val: 0,
                f64_val: 0.0,
                str: bytes.as_ptr(),
                str_len: bytes.len(),
                str_id: 0,
            },
            Scalar::StringId(id) => Self {
                kind: PF_SCALAR_STRING_ID,
                bool_val: 0,
                i64_val: 0,
                f64_val: 0.0,
                str: ptr::null(),
                str_len: 0,
                str_id: id,
            },
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PfHostVTable {
    pub make_null: Option<unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> HostFnResult>,
    pub make_bool: Option<unsafe extern "C" fn(*mut c_void, i32, *mut *mut c_void) -> HostFnResult>,
    pub make_i64: Option<unsafe extern "C" fn(*mut c_void, i64, *mut *mut c_void) -> HostFnResult>,
    pub make_f64: Option<unsafe extern "C" fn(*mut c_void, f64, *mut *mut c_void) -> HostFnResult>,
    pub make_string: Option<
        unsafe extern "C" fn(*mut c_void, *const u8, usize, *mut *mut c_void) -> HostFnResult,
    >,
    pub begin_array:
        Option<unsafe extern "C" fn(*mut c_void, i32, u32, *mut *mut c_void) -> HostFnResult>,
    pub push_packed: Option<unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> HostFnResult>,
    pub insert_i64_key:
        Option<unsafe extern "C" fn(*mut c_void, *mut c_void, i64, *mut c_void) -> HostFnResult>,
    pub insert_string_key: Option<
        unsafe extern "C" fn(*mut c_void, *mut c_void, *const u8, usize, *mut c_void) -> HostFnResult,
    >,
    pub end_array:
        Option<unsafe extern "C" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> HostFnResult>,
    pub drop_value: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
    pub drop_array: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
    pub push_scalar: Option<unsafe extern "C" fn(*mut c_void, *mut c_void, *const PfScalar) -> HostFnResult>,
    pub insert_scalar_i64_key:
        Option<unsafe extern "C" fn(*mut c_void, *mut c_void, i64, *const PfScalar) -> HostFnResult>,
    pub insert_scalar_string_key: Option<
        unsafe extern "C" fn(*mut c_void, *mut c_void, *const u8, usize, *const PfScalar) -> HostFnResult,
    >,
    pub push_i64_run:
        Option<unsafe extern "C" fn(*mut c_void, *mut c_void, *const i64, usize) -> HostFnResult>,
    /// Optional: cache string table entry as host-native string (zend_string).
    pub intern_string:
        Option<unsafe extern "C" fn(*mut c_void, u32, *const u8, usize) -> HostFnResult>,
    pub make_string_id:
        Option<unsafe extern "C" fn(*mut c_void, u32, *mut *mut c_void) -> HostFnResult>,
    pub insert_string_key_id:
        Option<unsafe extern "C" fn(*mut c_void, *mut c_void, u32, *mut c_void) -> HostFnResult>,
    pub insert_scalar_string_key_id: Option<
        unsafe extern "C" fn(*mut c_void, *mut c_void, u32, *const PfScalar) -> HostFnResult,
    >,
    pub duplicate_value:
        Option<unsafe extern "C" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> HostFnResult>,
}

impl PfHostVTable {
    pub fn has_required(&self) -> bool {
        self.make_null.is_some()
            && self.make_bool.is_some()
            && self.make_i64.is_some()
            && self.make_f64.is_some()
            && self.make_string.is_some()
            && self.begin_array.is_some()
            && self.push_packed.is_some()
            && self.insert_i64_key.is_some()
            && self.insert_string_key.is_some()
            && self.end_array.is_some()
    }
}

pub struct CHost<'a> {
    ctx: *mut c_void,
    vt: &'a PfHostVTable,
    /// Used when host omits intern_string / make_string_id (tests / incomplete adapters).
    fallback_strings: Vec<Vec<u8>>,
}

impl<'a> CHost<'a> {
    pub fn new(ctx: *mut c_void, vt: &'a PfHostVTable) -> Self {
        Self {
            ctx,
            vt,
            fallback_strings: Vec::new(),
        }
    }

    fn map_rc(rc: HostFnResult) -> Result<()> {
        if rc == 0 {
            Ok(())
        } else {
            Err(Error::Host)
        }
    }

    fn fallback_bytes(&self, id: u32) -> Result<&[u8]> {
        self.fallback_strings
            .get(id as usize)
            .map(|v| v.as_slice())
            .ok_or(Error::BadStrRef)
    }
}

pub struct CValue {
    ptr: *mut c_void,
    ctx: *mut c_void,
    drop_fn: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
}

impl Drop for CValue {
    fn drop(&mut self) {
        if let Some(f) = self.drop_fn {
            if !self.ptr.is_null() {
                unsafe { f(self.ctx, self.ptr) };
                self.ptr = ptr::null_mut();
            }
        }
    }
}

pub struct CArray {
    ptr: *mut c_void,
    ctx: *mut c_void,
    drop_fn: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
}

impl Drop for CArray {
    fn drop(&mut self) {
        if let Some(f) = self.drop_fn {
            if !self.ptr.is_null() {
                unsafe { f(self.ctx, self.ptr) };
                self.ptr = ptr::null_mut();
            }
        }
    }
}

impl<'a> Host for CHost<'a> {
    type Value = CValue;
    type ArrayState = CArray;

    fn make_null(&mut self) -> Result<Self::Value> {
        let mut out = ptr::null_mut();
        let f = self.vt.make_null.ok_or(Error::Internal)?;
        Self::map_rc(unsafe { f(self.ctx, &mut out) })?;
        Ok(CValue {
            ptr: out,
            ctx: self.ctx,
            drop_fn: self.vt.drop_value,
        })
    }

    fn make_bool(&mut self, v: bool) -> Result<Self::Value> {
        let mut out = ptr::null_mut();
        let f = self.vt.make_bool.ok_or(Error::Internal)?;
        Self::map_rc(unsafe { f(self.ctx, v as i32, &mut out) })?;
        Ok(CValue {
            ptr: out,
            ctx: self.ctx,
            drop_fn: self.vt.drop_value,
        })
    }

    fn make_i64(&mut self, v: i64) -> Result<Self::Value> {
        let mut out = ptr::null_mut();
        let f = self.vt.make_i64.ok_or(Error::Internal)?;
        Self::map_rc(unsafe { f(self.ctx, v, &mut out) })?;
        Ok(CValue {
            ptr: out,
            ctx: self.ctx,
            drop_fn: self.vt.drop_value,
        })
    }

    fn make_f64(&mut self, v: f64) -> Result<Self::Value> {
        let mut out = ptr::null_mut();
        let f = self.vt.make_f64.ok_or(Error::Internal)?;
        Self::map_rc(unsafe { f(self.ctx, v, &mut out) })?;
        Ok(CValue {
            ptr: out,
            ctx: self.ctx,
            drop_fn: self.vt.drop_value,
        })
    }

    fn make_string(&mut self, bytes: &[u8]) -> Result<Self::Value> {
        let mut out = ptr::null_mut();
        let f = self.vt.make_string.ok_or(Error::Internal)?;
        Self::map_rc(unsafe { f(self.ctx, bytes.as_ptr(), bytes.len(), &mut out) })?;
        Ok(CValue {
            ptr: out,
            ctx: self.ctx,
            drop_fn: self.vt.drop_value,
        })
    }

    fn make_string_id(&mut self, id: u32) -> Result<Self::Value> {
        if let Some(f) = self.vt.make_string_id {
            let mut out = ptr::null_mut();
            Self::map_rc(unsafe { f(self.ctx, id, &mut out) })?;
            return Ok(CValue {
                ptr: out,
                ctx: self.ctx,
                drop_fn: self.vt.drop_value,
            });
        }
        let bytes = self.fallback_bytes(id)?.to_vec();
        self.make_string(&bytes)
    }

    fn intern_string(&mut self, id: u32, bytes: &[u8]) -> Result<()> {
        if let Some(f) = self.vt.intern_string {
            return Self::map_rc(unsafe { f(self.ctx, id, bytes.as_ptr(), bytes.len()) });
        }
        if id as usize != self.fallback_strings.len() {
            return Err(Error::Internal);
        }
        self.fallback_strings.push(bytes.to_vec());
        Ok(())
    }

    fn begin_array(&mut self, packed: bool, count: u32) -> Result<Self::ArrayState> {
        let mut out = ptr::null_mut();
        let f = self.vt.begin_array.ok_or(Error::Internal)?;
        Self::map_rc(unsafe { f(self.ctx, packed as i32, count, &mut out) })?;
        Ok(CArray {
            ptr: out,
            ctx: self.ctx,
            drop_fn: self.vt.drop_array,
        })
    }

    fn push_packed(&mut self, arr: &mut Self::ArrayState, val: Self::Value) -> Result<()> {
        let f = self.vt.push_packed.ok_or(Error::Internal)?;
        let vptr = val.ptr;
        let mut val = val;
        val.ptr = ptr::null_mut();
        match Self::map_rc(unsafe { f(self.ctx, arr.ptr, vptr) }) {
            Ok(()) => Ok(()),
            Err(e) => {
                val.ptr = vptr;
                Err(e)
            }
        }
    }

    fn insert_key(
        &mut self,
        arr: &mut Self::ArrayState,
        key: ArrayKey<'_>,
        val: Self::Value,
    ) -> Result<()> {
        let vptr = val.ptr;
        let mut val = val;
        val.ptr = ptr::null_mut();
        let rc = match key {
            ArrayKey::Int(i) => {
                let f = self.vt.insert_i64_key.ok_or(Error::Internal)?;
                Self::map_rc(unsafe { f(self.ctx, arr.ptr, i, vptr) })
            }
            ArrayKey::String(s) => {
                let f = self.vt.insert_string_key.ok_or(Error::Internal)?;
                Self::map_rc(unsafe { f(self.ctx, arr.ptr, s.as_ptr(), s.len(), vptr) })
            }
            ArrayKey::StringId(id) => {
                if let Some(f) = self.vt.insert_string_key_id {
                    Self::map_rc(unsafe { f(self.ctx, arr.ptr, id, vptr) })
                } else {
                    let bytes = self.fallback_bytes(id)?.to_vec();
                    let f = self.vt.insert_string_key.ok_or(Error::Internal)?;
                    Self::map_rc(unsafe {
                        f(self.ctx, arr.ptr, bytes.as_ptr(), bytes.len(), vptr)
                    })
                }
            }
        };
        match rc {
            Ok(()) => Ok(()),
            Err(e) => {
                val.ptr = vptr;
                Err(e)
            }
        }
    }

    fn end_array(&mut self, arr: Self::ArrayState) -> Result<Self::Value> {
        let mut out = ptr::null_mut();
        let f = self.vt.end_array.ok_or(Error::Internal)?;
        let aptr = arr.ptr;
        let mut arr = arr;
        arr.ptr = ptr::null_mut();
        Self::map_rc(unsafe { f(self.ctx, aptr, &mut out) })?;
        Ok(CValue {
            ptr: out,
            ctx: self.ctx,
            drop_fn: self.vt.drop_value,
        })
    }

    fn duplicate_value(&mut self, val: &Self::Value) -> Result<Self::Value> {
        if let Some(f) = self.vt.duplicate_value {
            let mut out = ptr::null_mut();
            Self::map_rc(unsafe { f(self.ctx, val.ptr, &mut out) })?;
            return Ok(CValue {
                ptr: out,
                ctx: self.ctx,
                drop_fn: self.vt.drop_value,
            });
        }
        Err(Error::Internal)
    }

    fn push_scalar(&mut self, arr: &mut Self::ArrayState, scalar: Scalar<'_>) -> Result<()> {
        if let Some(f) = self.vt.push_scalar {
            let s = PfScalar::from_scalar(scalar);
            return Self::map_rc(unsafe { f(self.ctx, arr.ptr, &s) });
        }
        // Fallback for hosts without hot-path hooks.
        match scalar {
            Scalar::Null => {
                let v = self.make_null()?;
                self.push_packed(arr, v)
            }
            Scalar::Bool(b) => {
                let v = self.make_bool(b)?;
                self.push_packed(arr, v)
            }
            Scalar::I64(i) => {
                let v = self.make_i64(i)?;
                self.push_packed(arr, v)
            }
            Scalar::F64(x) => {
                let v = self.make_f64(x)?;
                self.push_packed(arr, v)
            }
            Scalar::String(bytes) => {
                let v = self.make_string(bytes)?;
                self.push_packed(arr, v)
            }
            Scalar::StringId(id) => {
                let v = self.make_string_id(id)?;
                self.push_packed(arr, v)
            }
        }
    }

    fn insert_scalar(
        &mut self,
        arr: &mut Self::ArrayState,
        key: ArrayKey<'_>,
        scalar: Scalar<'_>,
    ) -> Result<()> {
        let s = PfScalar::from_scalar(scalar);
        match key {
            ArrayKey::Int(i) => {
                if let Some(f) = self.vt.insert_scalar_i64_key {
                    return Self::map_rc(unsafe { f(self.ctx, arr.ptr, i, &s) });
                }
            }
            ArrayKey::String(k) => {
                if let Some(f) = self.vt.insert_scalar_string_key {
                    return Self::map_rc(unsafe {
                        f(self.ctx, arr.ptr, k.as_ptr(), k.len(), &s)
                    });
                }
            }
            ArrayKey::StringId(id) => {
                if let Some(f) = self.vt.insert_scalar_string_key_id {
                    return Self::map_rc(unsafe { f(self.ctx, arr.ptr, id, &s) });
                }
                let bytes = self.fallback_bytes(id)?.to_vec();
                if let Some(f) = self.vt.insert_scalar_string_key {
                    return Self::map_rc(unsafe {
                        f(self.ctx, arr.ptr, bytes.as_ptr(), bytes.len(), &s)
                    });
                }
            }
        }
        match scalar {
            Scalar::Null => {
                let v = self.make_null()?;
                self.insert_key(arr, key, v)
            }
            Scalar::Bool(b) => {
                let v = self.make_bool(b)?;
                self.insert_key(arr, key, v)
            }
            Scalar::I64(i) => {
                let v = self.make_i64(i)?;
                self.insert_key(arr, key, v)
            }
            Scalar::F64(x) => {
                let v = self.make_f64(x)?;
                self.insert_key(arr, key, v)
            }
            Scalar::String(bytes) => {
                let v = self.make_string(bytes)?;
                self.insert_key(arr, key, v)
            }
            Scalar::StringId(id) => {
                let v = self.make_string_id(id)?;
                self.insert_key(arr, key, v)
            }
        }
    }

    fn push_i64_run(&mut self, arr: &mut Self::ArrayState, values: &[i64]) -> Result<()> {
        if let Some(f) = self.vt.push_i64_run {
            return Self::map_rc(unsafe {
                f(self.ctx, arr.ptr, values.as_ptr(), values.len())
            });
        }
        for &v in values {
            self.push_scalar(arr, Scalar::I64(v))?;
        }
        Ok(())
    }
}

impl CValue {
    pub fn into_raw(mut self) -> *mut c_void {
        let p = self.ptr;
        self.ptr = ptr::null_mut();
        p
    }
}
