//! Status codes, error reporting and the panic boundary.

use std::ffi::{CStr, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

use eclipsed::lexer::Span;
use eclipsed::parser::ParseError;

use crate::memory::{EclString, ecl_string_free};

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EclStatus {
    Ok = 0,
    NullArgument = 1,
    InvalidUtf8 = 2,
    InvalidDialect = 3,
    TooLarge = 4,
    Parse = 5,
    Panic = 6,
}

/// A failure description. Must be zero-initialised before first use; a
/// message left from an earlier failure is released before being replaced.
#[repr(C)]
#[derive(Debug)]
pub struct EclError {
    /// An [`EclStatus`]; stored as an integer because the host owns it.
    pub status: u32,
    /// The offending byte range for parse and UTF-8 errors, otherwise empty.
    pub start: u32,
    pub end: u32,
    /// NUL-terminated; released by [`ecl_error_free`].
    pub message: *mut c_char,
}

pub(crate) struct Failure {
    pub(crate) status: EclStatus,
    pub(crate) span: Span,
    pub(crate) message: String,
}

impl Failure {
    pub(crate) fn new(status: EclStatus, message: impl Into<String>) -> Self {
        Failure {
            status,
            span: Span::default(),
            message: message.into(),
        }
    }
}

impl From<ParseError> for Failure {
    fn from(error: ParseError) -> Self {
        Failure {
            status: EclStatus::Parse,
            span: error.span(),
            message: error.to_string(),
        }
    }
}

pub(crate) type Outcome<T> = Result<T, Failure>;

/// Runs `f`, reporting its failure, or a caught panic, through `err`.
///
/// Every export that can fail funnels through here so that no panic unwinds
/// into the host.
pub(crate) fn guard(err: *mut EclError, f: impl FnOnce() -> Outcome<()>) -> EclStatus {
    let outcome = catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|payload| {
        let detail = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("unknown panic");
        Err(Failure::new(
            EclStatus::Panic,
            format!("internal error: {detail}"),
        ))
    });
    match outcome {
        Ok(()) => EclStatus::Ok,
        Err(failure) => {
            let status = failure.status;
            // SAFETY: every export requires `err` to be null or valid.
            if let Some(err) = unsafe { err.as_mut() } {
                report(err, failure);
            }
            status
        }
    }
}

fn report(err: &mut EclError, failure: Failure) {
    // SAFETY: `EclError` requires `message` to be null or ours.
    unsafe { release_c_string(err.message) };
    err.status = failure.status as u32;
    err.start = failure.span.start;
    err.end = failure.span.end;
    err.message = c_string(failure.message);
}

fn c_string(text: String) -> *mut c_char {
    EclString::new(text.replace('\0', "")).ptr
}

/// # Safety
///
/// `ptr` must be null or come from [`c_string`] and not have been freed.
unsafe fn release_c_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        // SAFETY: an `EclString` of `strlen` bytes, made by `c_string`.
        unsafe {
            let len = CStr::from_ptr(ptr).count_bytes();
            ecl_string_free(&mut EclString { ptr, len });
        }
    }
}

/// Releases the message held by `err` and zeroes it.
///
/// # Safety
///
/// `err` must be null or point to a valid [`EclError`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ecl_error_free(err: *mut EclError) {
    // SAFETY: guaranteed by the caller.
    if let Some(err) = unsafe { err.as_mut() } {
        // SAFETY: `EclError` requires `message` to be null or ours.
        unsafe { release_c_string(err.message) };
        *err = EclError {
            status: EclStatus::Ok as u32,
            start: 0,
            end: 0,
            message: ptr::null_mut(),
        };
    }
}
