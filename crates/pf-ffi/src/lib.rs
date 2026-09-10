//! C ABI for PFSerialize.

mod c_host;
mod encode_api;

use std::os::raw::c_void;
use std::ptr;
use std::slice;

use pf_core::{decode_with, DecodeOptions, Error};

use crate::c_host::{CHost, PfHostVTable};

pub use encode_api::*;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PfErr {
    Ok = 0,
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
    NullArg = 16,
    Internal = 127,
}

impl From<Error> for PfErr {
    fn from(e: Error) -> Self {
        match e {
            Error::BadMagic => PfErr::BadMagic,
            Error::UnsupportedVersion => PfErr::UnsupportedVersion,
            Error::UnknownFlags => PfErr::UnknownFlags,
            Error::Checksum => PfErr::Checksum,
            Error::Truncated => PfErr::Truncated,
            Error::Trailing => PfErr::Trailing,
            Error::UnknownTag => PfErr::UnknownTag,
            Error::BadKey => PfErr::BadKey,
            Error::VarInt => PfErr::VarInt,
            Error::BadStrRef => PfErr::BadStrRef,
            Error::MaxDepth => PfErr::MaxDepth,
            Error::MaxSize => PfErr::MaxSize,
            Error::Overflow => PfErr::Overflow,
            Error::UnsupportedType => PfErr::UnsupportedType,
            Error::Host => PfErr::Host,
            Error::Internal => PfErr::Internal,
        }
    }
}

/// Free a buffer returned by `pf_encoder_finish`.
#[no_mangle]
pub unsafe extern "C" fn pf_free(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    // Reconstruct Vec from raw parts written by finish (length unknown to free —
    // we use layout: actually we need to store capacity. Use libc malloc/free instead.)
    // See encode_api: we allocate with libc::malloc so use libc::free.
    libc_free(ptr as *mut c_void);
}

#[cfg(unix)]
unsafe fn libc_free(p: *mut c_void) {
    extern "C" {
        fn free(p: *mut c_void);
    }
    free(p);
}

#[cfg(windows)]
unsafe fn libc_free(p: *mut c_void) {
    extern "C" {
        fn free(p: *mut c_void);
    }
    free(p);
}

/// Decode PFBinary into a host-owned value.
///
/// # Safety
/// `host` must be valid for the duration; callbacks must uphold the vtable contract.
#[no_mangle]
pub unsafe extern "C" fn pf_decode(
    host: *const PfHostVTable,
    host_ctx: *mut c_void,
    data: *const u8,
    len: usize,
    out_value: *mut *mut c_void,
    max_depth: u32,
    max_size: usize,
) -> PfErr {
    if host.is_null() || out_value.is_null() {
        return PfErr::NullArg;
    }
    if len > 0 && data.is_null() {
        return PfErr::NullArg;
    }

    let vt = &*host;
    if !vt.has_required() {
        return PfErr::NullArg;
    }

    let bytes = if len == 0 {
        &[][..]
    } else {
        slice::from_raw_parts(data, len)
    };
    let mut chost = CHost::new(host_ctx, vt);
    let opts = DecodeOptions {
        max_depth,
        max_size,
        strict_trailing: true,
    };

    match decode_with(bytes, &mut chost, &opts) {
        Ok(v) => {
            *out_value = v.into_raw();
            PfErr::Ok
        }
        Err(e) => {
            *out_value = ptr::null_mut();
            e.into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pf_core::{encode, Value};
    use pf_format::FLAG_STRDEDUP;
    use std::cell::RefCell;

    #[derive(Debug, Clone)]
    #[allow(dead_code)]
    enum Tv {
        Null,
        Bool(bool),
        Int(i64),
        Float(f64),
        Str(Vec<u8>),
        Arr { packed: bool, items: Vec<(Option<i64>, Option<Vec<u8>>, Box<Tv>)> },
    }

    struct Arena {
        values: RefCell<Vec<Tv>>,
    }

    fn arena_get(arena: &Arena, p: *mut c_void) -> Tv {
        let idx = p as usize;
        arena.values.borrow()[idx].clone()
    }

    fn arena_push(arena: &Arena, v: Tv) -> *mut c_void {
        let mut g = arena.values.borrow_mut();
        let idx = g.len();
        g.push(v);
        idx as *mut c_void
    }

    unsafe extern "C" fn make_null(ctx: *mut c_void, out: *mut *mut c_void) -> i32 {
        let arena = &*(ctx as *const Arena);
        *out = arena_push(arena, Tv::Null);
        0
    }
    unsafe extern "C" fn make_bool(ctx: *mut c_void, v: i32, out: *mut *mut c_void) -> i32 {
        let arena = &*(ctx as *const Arena);
        *out = arena_push(arena, Tv::Bool(v != 0));
        0
    }
    unsafe extern "C" fn make_i64(ctx: *mut c_void, v: i64, out: *mut *mut c_void) -> i32 {
        let arena = &*(ctx as *const Arena);
        *out = arena_push(arena, Tv::Int(v));
        0
    }
    unsafe extern "C" fn make_f64(ctx: *mut c_void, v: f64, out: *mut *mut c_void) -> i32 {
        let arena = &*(ctx as *const Arena);
        *out = arena_push(arena, Tv::Float(v));
        0
    }
    unsafe extern "C" fn make_string(
        ctx: *mut c_void,
        bytes: *const u8,
        len: usize,
        out: *mut *mut c_void,
    ) -> i32 {
        let arena = &*(ctx as *const Arena);
        let s = slice::from_raw_parts(bytes, len).to_vec();
        *out = arena_push(arena, Tv::Str(s));
        0
    }
    unsafe extern "C" fn begin_array(
        ctx: *mut c_void,
        packed: i32,
        _count: u32,
        out: *mut *mut c_void,
    ) -> i32 {
        let arena = &*(ctx as *const Arena);
        *out = arena_push(
            arena,
            Tv::Arr {
                packed: packed != 0,
                items: Vec::new(),
            },
        );
        0
    }
    unsafe extern "C" fn push_packed(ctx: *mut c_void, array: *mut c_void, value: *mut c_void) -> i32 {
        let arena = &*(ctx as *const Arena);
        let vi = value as usize;
        let ai = array as usize;
        let val = arena.values.borrow()[vi].clone();
        if let Tv::Arr { items, .. } = &mut arena.values.borrow_mut()[ai] {
            items.push((None, None, Box::new(val)));
            0
        } else {
            1
        }
    }
    unsafe extern "C" fn insert_i64_key(
        ctx: *mut c_void,
        array: *mut c_void,
        key: i64,
        value: *mut c_void,
    ) -> i32 {
        let arena = &*(ctx as *const Arena);
        let vi = value as usize;
        let ai = array as usize;
        let val = arena.values.borrow()[vi].clone();
        if let Tv::Arr { items, .. } = &mut arena.values.borrow_mut()[ai] {
            items.push((Some(key), None, Box::new(val)));
            0
        } else {
            1
        }
    }
    unsafe extern "C" fn insert_string_key(
        ctx: *mut c_void,
        array: *mut c_void,
        key: *const u8,
        key_len: usize,
        value: *mut c_void,
    ) -> i32 {
        let arena = &*(ctx as *const Arena);
        let k = slice::from_raw_parts(key, key_len).to_vec();
        let vi = value as usize;
        let ai = array as usize;
        let val = arena.values.borrow()[vi].clone();
        if let Tv::Arr { items, .. } = &mut arena.values.borrow_mut()[ai] {
            items.push((None, Some(k), Box::new(val)));
            0
        } else {
            1
        }
    }
    unsafe extern "C" fn end_array(
        ctx: *mut c_void,
        array: *mut c_void,
        out: *mut *mut c_void,
    ) -> i32 {
        let _ = ctx;
        *out = array;
        0
    }

    #[test]
    fn ffi_decode_packed() {
        let v = Value::ArrayPacked(vec![Value::Int(1), Value::Int(2)]);
        let bytes = encode(&v).unwrap();
        let arena = Arena {
            values: RefCell::new(Vec::new()),
        };
        let vt = PfHostVTable {
            make_null: Some(make_null),
            make_bool: Some(make_bool),
            make_i64: Some(make_i64),
            make_f64: Some(make_f64),
            make_string: Some(make_string),
            begin_array: Some(begin_array),
            push_packed: Some(push_packed),
            insert_i64_key: Some(insert_i64_key),
            insert_string_key: Some(insert_string_key),
            end_array: Some(end_array),
            drop_value: None,
            drop_array: None,
            push_scalar: None,
            insert_scalar_i64_key: None,
            insert_scalar_string_key: None,
            push_i64_run: None,
            intern_string: None,
            make_string_id: None,
            insert_string_key_id: None,
            insert_scalar_string_key_id: None,
            duplicate_value: None,
        };
        let mut out: *mut c_void = ptr::null_mut();
        let err = unsafe {
            pf_decode(
                &vt,
                &arena as *const Arena as *mut c_void,
                bytes.as_ptr(),
                bytes.len(),
                &mut out,
                0,
                0,
            )
        };
        assert_eq!(err, PfErr::Ok);
        let root = arena_get(&arena, out);
        match root {
            Tv::Arr { packed, items } => {
                assert!(packed);
                assert_eq!(items.len(), 2);
            }
            _ => panic!("expected array"),
        }
        let _ = FLAG_STRDEDUP;
    }

    #[test]
    fn ffi_encoder_roundtrip_hex() {
        unsafe {
            let enc = pf_encoder_new(FLAG_STRDEDUP);
            assert!(!enc.is_null());
            assert_eq!(pf_encoder_packed_longs_begin(enc, 4), PfErr::Ok);
            assert_eq!(pf_encoder_packed_long_item(enc, 1), PfErr::Ok);
            assert_eq!(pf_encoder_packed_long_item(enc, 2), PfErr::Ok);
            assert_eq!(pf_encoder_packed_long_item(enc, 3), PfErr::Ok);
            assert_eq!(pf_encoder_packed_long_item(enc, 4), PfErr::Ok);
            let mut out: *mut u8 = ptr::null_mut();
            let mut len: usize = 0;
            assert_eq!(pf_encoder_finish(enc, &mut out, &mut len), PfErr::Ok);
            let slice = std::slice::from_raw_parts(out, len);
            assert_eq!(
                slice,
                &[0x50, 0x46, 0x42, 0x4E, 0x01, 0x02, 0x0A, 0x04, 0x02, 0x04, 0x06, 0x08,]
            );
            pf_free(out);
        }
    }
}
