//! Parse-free operations on the token stream.

use std::ffi::c_char;
use std::panic::catch_unwind;

use eclipsed::cst::{TokenRef, TriviaKind};
use eclipsed::lexer::{Lexer, Type};
use eclipsed::stream::{self, TokenKind, TokenStream};

use crate::error::{EclError, EclStatus, guard};
use crate::input::{dialect, source};
use crate::memory::{EclString, EclTokenArray, write};

/// Token ids beyond the lexer's types, which run from zero up to
/// `ECL_TRIVIA_WHITESPACE`.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EclTrivia {
    Whitespace = 83,
    Newline = 84,
    LineComment = 85,
    LongComment = 86,
    CComment = 87,
    Shebang = 88,
}

const _: () = assert!(EclTrivia::Whitespace as usize == Type::ALL.len());

impl EclTrivia {
    pub(crate) const NAMES: [&str; 6] = [
        "Whitespace",
        "Newline",
        "LineComment",
        "LongComment",
        "CComment",
        "Shebang",
    ];
}

impl From<TriviaKind> for EclTrivia {
    fn from(kind: TriviaKind) -> Self {
        match kind {
            TriviaKind::Whitespace => EclTrivia::Whitespace,
            TriviaKind::Newline => EclTrivia::Newline,
            TriviaKind::LineComment => EclTrivia::LineComment,
            TriviaKind::LongComment => EclTrivia::LongComment,
            TriviaKind::CStyleComment => EclTrivia::CComment,
            TriviaKind::Shebang => EclTrivia::Shebang,
        }
    }
}

/// One token as a byte range into the source.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EclToken {
    /// A lexer type or an [`EclTrivia`], named by `ecl_token_type_name`.
    pub token_type: u32,
    pub start: u32,
    pub end: u32,
}

/// Flattens trivia-carrying tokens into the stream `ecl_lex` returns.
pub(crate) fn flatten(tokens: &[TokenRef<'_>]) -> Vec<EclToken> {
    let mut out = Vec::with_capacity(tokens.len() * 2);
    out.extend(stream::flatten(tokens).map(|(kind, span)| EclToken {
        token_type: match kind {
            TokenKind::Syntax(token_type) => token_type as u32,
            TokenKind::Trivia(trivia) => EclTrivia::from(trivia) as u32,
        },
        start: span.start,
        end: span.end,
    }));
    out
}

/// Lexes `src` into tokens, trivia included, whose ranges tile the source.
/// Malformed code produces error tokens rather than failing.
///
/// # Safety
///
/// `src` must point to `len` readable bytes, or be null with `len` zero;
/// `out` must be valid for writes; `err` must be null or point to a valid
/// [`EclError`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ecl_lex(
    src: *const c_char,
    len: usize,
    dialect_id: u32,
    out: *mut EclTokenArray,
    err: *mut EclError,
) -> EclStatus {
    guard(err, || {
        // SAFETY: guaranteed by the caller.
        let source = unsafe { source(src, len) }?;
        let arena = bumpalo::Bump::new();
        let tokens = flatten(Lexer::new(source, &arena, dialect(dialect_id)?).lex_cst());
        // SAFETY: guaranteed by the caller.
        unsafe { write(out, || EclTokenArray::new(tokens)) }
    })
}

/// Removes all trivia, keeping a space only where two tokens would fuse.
///
/// # Safety
///
/// As [`ecl_lex`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ecl_minify(
    src: *const c_char,
    len: usize,
    dialect_id: u32,
    out: *mut EclString,
    err: *mut EclError,
) -> EclStatus {
    guard(err, || {
        // SAFETY: guaranteed by the caller.
        let source = unsafe { source(src, len) }?;
        let dialect = dialect(dialect_id)?;
        let mut tokens = TokenStream::lex(source, dialect);
        stream::strip_trivia(&mut tokens, dialect);
        // SAFETY: guaranteed by the caller.
        unsafe { write(out, || EclString::new(tokens.render())) }
    })
}

/// Whether tokens `a` and `b` would lex differently written adjacently:
/// 1 if they need a separator, 0 if not, -1 for invalid arguments.
///
/// # Safety
///
/// `a` and `b` must each point to their length in readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ecl_needs_space(
    a: *const c_char,
    a_len: usize,
    b: *const c_char,
    b_len: usize,
    dialect_id: u32,
) -> i32 {
    catch_unwind(|| {
        // SAFETY: guaranteed by the caller.
        let (a, b) = unsafe { (source(a, a_len), source(b, b_len)) };
        match (a, b, dialect(dialect_id)) {
            (Ok(a), Ok(b), Ok(dialect)) => i32::from(stream::would_fuse(a, b, dialect)),
            _ => -1,
        }
    })
    // -1 is truthy, so a careless caller still inserts a separator.
    .unwrap_or(-1)
}
