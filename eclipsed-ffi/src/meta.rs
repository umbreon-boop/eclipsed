//! Versions and the names behind numeric ids.

use std::ffi::{CStr, CString, c_char};
use std::ptr;
use std::sync::OnceLock;

use eclipsed::dialect::Dialect;
use eclipsed::lexer::Type;

use crate::input::EclDialect;
use crate::lex::EclTrivia;

/// Bumped on every incompatible change to this interface.
pub const ECL_ABI_VERSION: u32 = 2;

/// The version of this interface; see [`ECL_ABI_VERSION`].
#[unsafe(no_mangle)]
pub extern "C" fn ecl_abi_version() -> u32 {
    ECL_ABI_VERSION
}

/// The library version, as a static string.
#[unsafe(no_mangle)]
pub extern "C" fn ecl_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

/// The [`EclDialect`] named `name`, such as `"luau"`, or -1.
///
/// # Safety
///
/// `name` must be null or a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ecl_dialect_from_name(name: *const c_char) -> i32 {
    if name.is_null() {
        return -1;
    }
    // SAFETY: guaranteed by the caller.
    let name = unsafe { CStr::from_ptr(name) };
    name.to_str()
        .ok()
        .and_then(|name| name.parse::<Dialect>().ok())
        .map_or(-1, |dialect| EclDialect::from(dialect) as i32)
}

/// The static name of a dialect id, or null.
#[unsafe(no_mangle)]
pub extern "C" fn ecl_dialect_name(id: u32) -> *const c_char {
    match EclDialect::from_id(id) {
        Some(EclDialect::Lua) => c"lua".as_ptr(),
        Some(EclDialect::Luau) => c"luau".as_ptr(),
        Some(EclDialect::LuaJit) => c"luajit".as_ptr(),
        Some(EclDialect::CfxLua) => c"cfxlua".as_ptr(),
        None => ptr::null(),
    }
}

/// The number of token ids, trivia included; ids run from zero up to this.
#[unsafe(no_mangle)]
pub extern "C" fn ecl_token_type_count() -> u32 {
    (Type::ALL.len() + EclTrivia::NAMES.len()) as u32
}

/// The static name of a token id, such as `"Identifier"` or `"Newline"`, or
/// null.
#[unsafe(no_mangle)]
pub extern "C" fn ecl_token_type_name(id: u32) -> *const c_char {
    static NAMES: OnceLock<Vec<CString>> = OnceLock::new();
    let names = NAMES.get_or_init(|| {
        Type::ALL
            .iter()
            .map(|kind| kind.name())
            .chain(EclTrivia::NAMES)
            .map(|name| CString::new(name).unwrap())
            .collect()
    });
    names
        .get(id as usize)
        .map_or(ptr::null(), |name| name.as_ptr())
}
