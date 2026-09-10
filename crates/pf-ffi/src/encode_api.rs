//! Fine-grained C encoder API.

use std::os::raw::c_void;
use std::ptr;
use std::slice;

use pf_core::Encoder;

use crate::PfErr;

pub struct PfEncoder {
    inner: Encoder,
}

#[no_mangle]
pub unsafe extern "C" fn pf_encoder_new(flags: u8) -> *mut PfEncoder {
    match Encoder::new(flags) {
        Ok(inner) => Box::into_raw(Box::new(PfEncoder { inner })),
        Err(_) => ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn pf_encoder_free(enc: *mut PfEncoder) {
    if enc.is_null() {
        return;
    }
    drop(Box::from_raw(enc));
}

macro_rules! with_enc {
    ($enc:expr, $body:expr) => {{
        if $enc.is_null() {
            PfErr::NullArg
        } else {
            let e = &mut (*$enc).inner;
            $body(e);
            PfErr::Ok
        }
    }};
}

#[no_mangle]
pub unsafe extern "C" fn pf_encoder_null(enc: *mut PfEncoder) -> PfErr {
    with_enc!(enc, |e: &mut Encoder| e.write_null())
}

#[no_mangle]
pub unsafe extern "C" fn pf_encoder_bool(enc: *mut PfEncoder, v: i32) -> PfErr {
    with_enc!(enc, |e: &mut Encoder| e.write_bool(v != 0))
}

#[no_mangle]
pub unsafe extern "C" fn pf_encoder_i64(enc: *mut PfEncoder, v: i64) -> PfErr {
    with_enc!(enc, |e: &mut Encoder| e.write_i64(v))
}

#[no_mangle]
pub unsafe extern "C" fn pf_encoder_f64(enc: *mut PfEncoder, v: f64) -> PfErr {
    with_enc!(enc, |e: &mut Encoder| e.write_f64(v))
}

#[no_mangle]
pub unsafe extern "C" fn pf_encoder_string(
    enc: *mut PfEncoder,
    bytes: *const u8,
    len: usize,
) -> PfErr {
    if len > 0 && bytes.is_null() {
        return PfErr::NullArg;
    }
    let s = if len == 0 {
        &[][..]
    } else {
        slice::from_raw_parts(bytes, len)
    };
    with_enc!(enc, |e: &mut Encoder| e.write_string(s))
}

#[no_mangle]
pub unsafe extern "C" fn pf_encoder_array_packed_begin(enc: *mut PfEncoder, count: u32) -> PfErr {
    with_enc!(enc, |e: &mut Encoder| e.write_array_packed_begin(count))
}

#[no_mangle]
pub unsafe extern "C" fn pf_encoder_packed_longs_begin(enc: *mut PfEncoder, count: u32) -> PfErr {
    with_enc!(enc, |e: &mut Encoder| e.write_packed_longs_begin(count))
}

#[no_mangle]
pub unsafe extern "C" fn pf_encoder_packed_long_item(enc: *mut PfEncoder, v: i64) -> PfErr {
    with_enc!(enc, |e: &mut Encoder| e.write_packed_long_item(v))
}

#[no_mangle]
pub unsafe extern "C" fn pf_encoder_array_hash_begin(enc: *mut PfEncoder, count: u32) -> PfErr {
    with_enc!(enc, |e: &mut Encoder| e.write_array_hash_begin(count))
}

#[no_mangle]
pub unsafe extern "C" fn pf_encoder_array_rows_begin(
    enc: *mut PfEncoder,
    nrows: u32,
    ncols: u32,
) -> PfErr {
    with_enc!(enc, |e: &mut Encoder| e.write_array_rows_begin(nrows, ncols))
}

#[no_mangle]
pub unsafe extern "C" fn pf_encoder_array_repeat_begin(enc: *mut PfEncoder, count: u32) -> PfErr {
    with_enc!(enc, |e: &mut Encoder| e.write_array_repeat_begin(count))
}

#[no_mangle]
pub unsafe extern "C" fn pf_encoder_key_i64(enc: *mut PfEncoder, key: i64) -> PfErr {
    with_enc!(enc, |e: &mut Encoder| e.write_key_i64(key))
}

#[no_mangle]
pub unsafe extern "C" fn pf_encoder_key_string(
    enc: *mut PfEncoder,
    bytes: *const u8,
    len: usize,
) -> PfErr {
    if len > 0 && bytes.is_null() {
        return PfErr::NullArg;
    }
    let s = if len == 0 {
        &[][..]
    } else {
        slice::from_raw_parts(bytes, len)
    };
    with_enc!(enc, |e: &mut Encoder| e.write_key_string(s))
}

#[no_mangle]
pub unsafe extern "C" fn pf_encoder_finish(
    enc: *mut PfEncoder,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> PfErr {
    if enc.is_null() || out.is_null() || out_len.is_null() {
        if !enc.is_null() {
            drop(Box::from_raw(enc));
        }
        return PfErr::NullArg;
    }

    let boxed = Box::from_raw(enc);
    let bytes = boxed.inner.finish();
    let len = bytes.len();

    let alloc_len = len.max(1);
    let buf = libc_malloc(alloc_len) as *mut u8;
    if buf.is_null() {
        return PfErr::Internal;
    }
    if len > 0 {
        ptr::copy_nonoverlapping(bytes.as_ptr(), buf, len);
    }
    *out = buf;
    *out_len = len;
    PfErr::Ok
}

unsafe fn libc_malloc(n: usize) -> *mut c_void {
    extern "C" {
        fn malloc(n: usize) -> *mut c_void;
    }
    malloc(n)
}
