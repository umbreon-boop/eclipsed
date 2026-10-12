//! Owned buffers handed across the boundary and their release functions.

use std::ffi::c_char;
use std::ptr;

use crate::error::{EclStatus, Failure, Outcome};
use crate::lex::EclToken;

/// An owned byte string; `ptr[len]` is a NUL byte.
#[repr(C)]
#[derive(Debug)]
pub struct EclString {
    pub ptr: *mut c_char,
    pub len: usize,
}

impl EclString {
    pub(crate) fn new(text: String) -> Self {
        let len = text.len();
        let mut bytes = text.into_bytes();
        bytes.push(0);
        let (ptr, _) = into_raw_slice(bytes);
        EclString {
            ptr: ptr.cast(),
            len,
        }
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct EclTokenArray {
    pub ptr: *mut EclToken,
    pub len: usize,
}

impl EclTokenArray {
    pub(crate) fn new(tokens: Vec<EclToken>) -> Self {
        let (ptr, len) = into_raw_slice(tokens);
        EclTokenArray { ptr, len }
    }
}

fn into_raw_slice<T>(items: Vec<T>) -> (*mut T, usize) {
    let len = items.len();
    (Box::into_raw(items.into_boxed_slice()).cast(), len)
}

/// # Safety
///
/// `ptr` must be null, or `ptr` and `len` must come from [`into_raw_slice`]
/// and not have been freed.
unsafe fn free_raw_slice<T>(ptr: *mut T, len: usize) {
    if !ptr.is_null() {
        // SAFETY: guaranteed by the caller.
        drop(unsafe { Box::from_raw(ptr::slice_from_raw_parts_mut(ptr, len)) });
    }
}

/// Writes `make()` through `out`, failing before ownership passes to the
/// host if `out` is null.
///
/// # Safety
///
/// `out` must be null or valid for writes.
pub(crate) unsafe fn write<T>(out: *mut T, make: impl FnOnce() -> T) -> Outcome<()> {
    if out.is_null() {
        return Err(Failure::new(
            EclStatus::NullArgument,
            "output pointer is null",
        ));
    }
    // SAFETY: guaranteed by the caller.
    unsafe { out.write(make()) };
    Ok(())
}

/// Releases `string` and zeroes it.
///
/// # Safety
///
/// `string` must be null or point to a zeroed [`EclString`] or one produced
/// by this library.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ecl_string_free(string: *mut EclString) {
    // SAFETY: guaranteed by the caller.
    if let Some(string) = unsafe { string.as_mut() } {
        // SAFETY: `EclString::new` allocates `len + 1` bytes.
        unsafe { free_raw_slice(string.ptr.cast::<u8>(), string.len + 1) };
        string.ptr = ptr::null_mut();
        string.len = 0;
    }
}

/// Releases `tokens` and zeroes it.
///
/// # Safety
///
/// `tokens` must be null or point to a zeroed [`EclTokenArray`] or one
/// produced by this library.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ecl_token_array_free(tokens: *mut EclTokenArray) {
    // SAFETY: guaranteed by the caller.
    if let Some(tokens) = unsafe { tokens.as_mut() } {
        // SAFETY: produced by `EclTokenArray::new`.
        unsafe { free_raw_slice(tokens.ptr, tokens.len) };
        tokens.ptr = ptr::null_mut();
        tokens.len = 0;
    }
}
