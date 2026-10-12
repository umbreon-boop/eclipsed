//! C ABI for `eclipsed`.
//!
//! Every fallible function returns an [`EclStatus`] and, when `err` is not
//! null, describes the failure in `*err`. Outputs are written through
//! out-pointers only on success. Memory handed out is released with the
//! matching `*_free` function, all of which accept already-freed values.
//!
//! Source text is passed as a pointer and byte length; it need not be
//! NUL-terminated, and all offsets reported back are byte offsets into it.

mod chunk;
mod decls;
mod error;
mod input;
mod lex;
mod memory;
mod meta;
#[cfg(test)]
mod tests;
mod tree;

pub use chunk::*;
pub use decls::{EclDecl, EclDeclKind, EclNameJoin, EclRecord};
pub use error::*;
pub use input::EclDialect;
pub use lex::*;
pub use memory::*;
pub use meta::*;
pub use tree::*;
