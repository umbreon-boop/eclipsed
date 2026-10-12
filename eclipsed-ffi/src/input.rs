//! Validation of arguments coming in from the host.

use std::ffi::c_char;

use eclipsed::dialect::Dialect;
use eclipsed::lexer::Span;

use crate::error::{EclStatus, Failure, Outcome};

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EclDialect {
    Lua = 0,
    Luau = 1,
    LuaJit = 2,
    CfxLua = 3,
}

impl EclDialect {
    pub(crate) fn from_id(id: u32) -> Option<Self> {
        Some(match id {
            0 => EclDialect::Lua,
            1 => EclDialect::Luau,
            2 => EclDialect::LuaJit,
            3 => EclDialect::CfxLua,
            _ => return None,
        })
    }
}

impl From<EclDialect> for Dialect {
    fn from(dialect: EclDialect) -> Self {
        match dialect {
            EclDialect::Lua => Dialect::Lua,
            EclDialect::Luau => Dialect::Luau,
            EclDialect::LuaJit => Dialect::LuaJit,
            EclDialect::CfxLua => Dialect::CfxLua,
        }
    }
}

impl From<Dialect> for EclDialect {
    fn from(dialect: Dialect) -> Self {
        match dialect {
            Dialect::Lua => EclDialect::Lua,
            Dialect::Luau => EclDialect::Luau,
            Dialect::LuaJit => EclDialect::LuaJit,
            Dialect::CfxLua => EclDialect::CfxLua,
        }
    }
}

/// Dialects cross the boundary as plain integers: an out-of-range value in
/// a Rust enum would be undefined behaviour.
pub(crate) fn dialect(id: u32) -> Outcome<Dialect> {
    EclDialect::from_id(id).map(Dialect::from).ok_or_else(|| {
        Failure::new(
            EclStatus::InvalidDialect,
            format!("unknown dialect id {id}"),
        )
    })
}

/// # Safety
///
/// `src` must be null or point to `len` readable bytes that outlive `'a`.
pub(crate) unsafe fn source<'a>(src: *const c_char, len: usize) -> Outcome<&'a str> {
    if src.is_null() {
        return if len == 0 {
            Ok("")
        } else {
            Err(Failure::new(
                EclStatus::NullArgument,
                "source pointer is null",
            ))
        };
    }
    if u32::try_from(len).is_err() {
        return Err(Failure::new(EclStatus::TooLarge, "source exceeds 4 GiB"));
    }
    // SAFETY: guaranteed by the caller.
    let bytes = unsafe { std::slice::from_raw_parts(src.cast::<u8>(), len) };
    std::str::from_utf8(bytes).map_err(|error| {
        let offset = error.valid_up_to() as u32;
        let width = error.error_len().unwrap_or(0) as u32;
        Failure {
            status: EclStatus::InvalidUtf8,
            span: Span::new(offset, offset + width),
            message: format!("source is not valid UTF-8: {error}"),
        }
    })
}
