//! Parsing, and the chunk handle that holds a parse's results.

use std::ffi::c_char;
use std::ptr;

use eclipsed::api::{self, Chunk};
use eclipsed::typespan::TypeSpanKind;

use crate::decls::{self, EclDecl};
use crate::error::{EclError, EclStatus, guard};
use crate::input::{dialect, source};
use crate::lex::{EclToken, flatten};
use crate::memory::write;
use crate::tree::{Builder, EclList, EclStmt};

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EclTypeSpanKind {
    /// A type annotation, excluding its introducing `:` or `::`.
    Annotation = 0,
    /// A whole type alias or type function statement.
    AliasStatement = 1,
    /// A declared generic parameter's name.
    Parameter = 2,
}

/// One outermost occurrence of Luau type syntax.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EclTypeSpan {
    pub kind: EclTypeSpanKind,
    pub start: u32,
    pub end: u32,
}

/// A parsed chunk. Everything it hands out lives as long as it does.
#[derive(Debug)]
pub struct EclChunk {
    /// Owns every tree node `statements` reaches.
    _arena: bumpalo::Bump,
    statements: EclList<EclStmt>,
    tokens: Box<[EclToken]>,
    type_spans: Box<[EclTypeSpan]>,
    declarations: Box<[EclDecl]>,
}

fn type_spans(chunk: &Chunk<'_>) -> Vec<EclTypeSpan> {
    chunk
        .type_spans()
        .into_iter()
        .map(|entry| EclTypeSpan {
            kind: match entry.kind {
                TypeSpanKind::Annotation => EclTypeSpanKind::Annotation,
                TypeSpanKind::AliasStatement => EclTypeSpanKind::AliasStatement,
                TypeSpanKind::Parameter => EclTypeSpanKind::Parameter,
            },
            start: entry.span.start,
            end: entry.span.end,
        })
        .collect()
}

/// Checks that `src` parses.
///
/// # Safety
///
/// `src` must point to `len` readable bytes, or be null with `len` zero, and
/// `err` must be null or point to a valid [`EclError`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ecl_check(
    src: *const c_char,
    len: usize,
    dialect_id: u32,
    err: *mut EclError,
) -> EclStatus {
    guard(err, || {
        // SAFETY: guaranteed by the caller.
        let source = unsafe { source(src, len) }?;
        api::with_chunk(source, dialect(dialect_id)?, |_| ())?;
        Ok(())
    })
}

/// Parses `src`, storing a chunk to be released with [`ecl_chunk_free`].
///
/// # Safety
///
/// As [`ecl_check`], and `out` must be valid for writes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ecl_parse(
    src: *const c_char,
    len: usize,
    dialect_id: u32,
    out: *mut *mut EclChunk,
    err: *mut EclError,
) -> EclStatus {
    guard(err, || {
        // SAFETY: guaranteed by the caller.
        let source = unsafe { source(src, len) }?;
        let dialect = dialect(dialect_id)?;
        let chunk = api::with_chunk(source, dialect, |chunk| {
            let arena = bumpalo::Bump::new();
            let statements = Builder {
                arena: &arena,
                source,
            }
            .block(chunk.statements());
            EclChunk {
                _arena: arena,
                statements,
                tokens: flatten(chunk.tokens()).into_boxed_slice(),
                type_spans: if dialect.has_types() {
                    type_spans(chunk).into_boxed_slice()
                } else {
                    Box::default()
                },
                declarations: decls::collect(chunk).into_boxed_slice(),
            }
        })?;
        // SAFETY: guaranteed by the caller.
        unsafe { write(out, || Box::into_raw(Box::new(chunk))) }
    })
}

/// Releases `chunk`.
///
/// # Safety
///
/// `chunk` must be null or come from [`ecl_parse`] and not have been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ecl_chunk_free(chunk: *mut EclChunk) {
    if !chunk.is_null() {
        // SAFETY: guaranteed by the caller.
        drop(unsafe { Box::from_raw(chunk) });
    }
}

/// # Safety
///
/// `chunk` must be null or live, and `len` null or valid for writes.
unsafe fn borrowed<T>(
    chunk: *const EclChunk,
    len: *mut usize,
    field: fn(&EclChunk) -> &[T],
) -> *const T {
    // SAFETY: guaranteed by the caller.
    let items = unsafe { chunk.as_ref() }.map_or(&[][..], field);
    if !len.is_null() {
        // SAFETY: guaranteed by the caller.
        unsafe { len.write(items.len()) };
    }
    if items.is_empty() {
        ptr::null()
    } else {
        items.as_ptr()
    }
}

/// The chunk's top-level statements, the root of its syntax tree; null
/// when empty.
///
/// # Safety
///
/// `chunk` must be null or live, and `len` null or valid for writes. The
/// tree lives until the chunk is freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ecl_chunk_statements(
    chunk: *const EclChunk,
    len: *mut usize,
) -> *const EclStmt {
    fn statements(chunk: &EclChunk) -> &[EclStmt] {
        // SAFETY: built from a slice of the chunk's own arena.
        unsafe { chunk.statements.as_slice() }
    }
    // SAFETY: guaranteed by the caller.
    unsafe { borrowed(chunk, len, statements) }
}

/// The chunk's tokens, as `ecl_lex` produces them; null when empty.
///
/// # Safety
///
/// `chunk` must be null or live, and `len` null or valid for writes. The
/// result lives until the chunk is freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ecl_chunk_tokens(
    chunk: *const EclChunk,
    len: *mut usize,
) -> *const EclToken {
    // SAFETY: guaranteed by the caller.
    unsafe { borrowed(chunk, len, |chunk| &chunk.tokens) }
}

/// Outermost Luau type syntax, sorted by start; empty without types.
///
/// # Safety
///
/// As [`ecl_chunk_tokens`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ecl_chunk_type_spans(
    chunk: *const EclChunk,
    len: *mut usize,
) -> *const EclTypeSpan {
    // SAFETY: guaranteed by the caller.
    unsafe { borrowed(chunk, len, |chunk| &chunk.type_spans) }
}

/// Module-level declarations, as rows described by [`EclDecl`].
///
/// # Safety
///
/// As [`ecl_chunk_tokens`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ecl_chunk_declarations(
    chunk: *const EclChunk,
    len: *mut usize,
) -> *const EclDecl {
    // SAFETY: guaranteed by the caller.
    unsafe { borrowed(chunk, len, |chunk| &chunk.declarations) }
}
