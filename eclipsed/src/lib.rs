//! A lossless lexer and parser for Lua 5.4, Luau, LuaJIT and CfxLua.
//!
#![cfg_attr(
    feature = "cst",
    doc = "[`api::with_chunk`] is the convenient entry point; [`lexer`] and [`parser`] are the underlying pieces."
)]
#![cfg_attr(
    not(feature = "cst"),
    doc = "[`lexer`] and [`parser`] are the underlying pieces."
)]
//! Dialects other than Lua and the trivia-carrying token stream are behind
//! cargo features.

#![deny(unsafe_code)]

#[cfg(feature = "cst")]
pub mod api;
pub mod ast;
#[cfg(feature = "cst")]
pub mod cst;
#[cfg(feature = "cst")]
pub mod declarations;
pub mod dialect;
pub mod interner;
pub mod lexer;
pub mod parser;
#[cfg(feature = "cst")]
pub mod stream;
pub mod string;
#[cfg(all(feature = "cst", feature = "luau"))]
pub mod typespan;

pub use dialect::Dialect;
pub use parser::ParseError;
