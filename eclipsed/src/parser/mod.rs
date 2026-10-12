//! Recursive-descent parser producing the [`crate::ast`].
//!
#![cfg_attr(
    feature = "cst",
    doc = "With the `cst` feature, [`crate::api::with_chunk`] owns the arena, interner and lexer for you; without it, build those yourself."
)]
#![cfg_attr(not(feature = "cst"), doc = "Build the arena and interner yourself.")]
//! Then drive [`Lexer`](crate::lexer::Lexer) and [`Parser`] directly:
//!
//! ```
//! use bumpalo::Bump;
//! use eclipsed::{Dialect, interner::Interner, lexer::Lexer, parser::Parser};
//!
//! let source = "local x = 1 -- hi\nreturn x";
//! let arena = Bump::new();
//! let tokens = Lexer::new(source, &arena, Dialect::Lua).lex();
//! let mut interner = Interner::new();
//! let statements = Parser::new(tokens, source, &arena, &mut interner, Dialect::Lua)
//!     .parse()
//!     .unwrap();
//! assert_eq!(statements.len(), 2);
//! ```

use bumpalo::Bump;
use thiserror::Error;

use crate::ast::{Statement, TypeAnnotation};
use crate::dialect::Dialect;
use crate::interner::{Interner, Symbol};
use crate::lexer::{Span, Token, Type};

#[cfg(feature = "compound")]
mod compound;
mod expression;
mod function;
#[cfg(feature = "luajit")]
mod luajit;
#[cfg(feature = "luau")]
mod luau;
mod number;
mod statement;
#[cfg(feature = "luau")]
mod types;

/// How deeply blocks, expressions and types may nest before parsing fails
/// with [`ParseError::TooDeep`].
pub const MAX_DEPTH: u32 = 1000;

/// Runs `f`, first growing the stack if little of it remains, so that a
/// recursive walk over any tree the parser accepts cannot overflow.
#[inline]
pub fn ensure_stack<R>(f: impl FnOnce() -> R) -> R {
    stacker::maybe_grow(128 * 1024, 2 * 1024 * 1024, f)
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum ParseError {
    #[error("{span}: expected {expected}, got {got}")]
    Expected {
        expected: &'static str,
        got: String,
        span: Span,
    },
    #[error("{span}: unexpected {token}")]
    Unexpected { token: String, span: Span },
    #[error("{span}: invalid variable attribute '{name}'")]
    InvalidAttribute { name: String, span: Span },
    #[error("{span}: nesting exceeds the limit of {MAX_DEPTH}")]
    TooDeep { span: Span },
    #[error("{span}: unexpected end of input")]
    EndOfInput { span: Span },
}

impl ParseError {
    pub fn span(&self) -> Span {
        match self {
            ParseError::Expected { span, .. }
            | ParseError::Unexpected { span, .. }
            | ParseError::InvalidAttribute { span, .. }
            | ParseError::TooDeep { span }
            | ParseError::EndOfInput { span } => *span,
        }
    }
}

#[derive(Debug)]
pub struct Parser<'source, 'arena, 'interner> {
    pub(crate) tokens: &'arena [Token],
    pub(crate) position: usize,
    pub(crate) source: &'source str,
    pub(crate) arena: &'arena Bump,
    pub(crate) interner: &'interner mut Interner<'source>,
    pub dialect: Dialect,
    /// Offset reported for errors at the end of the token stream.
    eof: u32,
    depth: u32,
    /// Position of a `>>` whose first `>` already closed a generic.
    #[cfg(feature = "luau")]
    pub(crate) pending_angle: Option<usize>,
    /// `@attribute`s awaiting the function they annotate.
    #[cfg(feature = "luau")]
    pub(crate) pending_attributes: Option<&'arena [crate::ast::SpannedSymbol]>,
    /// Allows the next parenthesized type to be a tuple pack.
    #[cfg(feature = "luau")]
    pub(crate) pack_tuple_ok: bool,
    /// Source offsets of `<<` already found not to start an instantiation,
    /// shared with nested parsers, so that nested attempts cannot backtrack
    /// exponentially.
    #[cfg(feature = "luau")]
    pub(crate) failed_instantiations: rustc_hash::FxHashSet<u32>,
}

impl<'source, 'arena, 'interner> Parser<'source, 'arena, 'interner> {
    pub fn new(
        tokens: &'arena [Token],
        source: &'source str,
        arena: &'arena Bump,
        interner: &'interner mut Interner<'source>,
        dialect: Dialect,
    ) -> Self {
        Self {
            tokens,
            position: 0,
            source,
            arena,
            interner,
            dialect,
            eof: source.len() as u32,
            depth: 0,
            #[cfg(feature = "luau")]
            pending_angle: None,
            #[cfg(feature = "luau")]
            pending_attributes: None,
            #[cfg(feature = "luau")]
            pack_tuple_ok: false,
            #[cfg(feature = "luau")]
            failed_instantiations: Default::default(),
        }
    }

    /// Builds a parser over a trivia-carrying token stream. Token `i` of the
    /// parser is token `i` of `cst_tokens`.
    #[cfg(feature = "cst")]
    pub fn new_cst(
        cst_tokens: &'arena [crate::cst::TokenRef<'arena>],
        source: &'source str,
        arena: &'arena Bump,
        interner: &'interner mut Interner<'source>,
        dialect: Dialect,
    ) -> Self {
        let body = match cst_tokens.split_last() {
            Some((last, body)) if last.token_type == Type::EndOfFile => body,
            _ => cst_tokens,
        };
        let plain = arena.alloc_slice_fill_iter(body.iter().map(|t| Token {
            token_type: t.token_type,
            span: t.span,
        }));
        Self::new(plain, source, arena, interner, dialect)
    }

    /// Parses a sub-range of the source, such as an interpolation hole.
    #[cfg(feature = "luau")]
    pub(crate) fn nested_parser<'a>(
        &'a mut self,
        tokens: &'arena [Token],
        eof: u32,
    ) -> Parser<'source, 'arena, 'a> {
        let mut sub = Parser::new(
            tokens,
            self.source,
            self.arena,
            &mut *self.interner,
            self.dialect,
        );
        sub.eof = eof;
        sub.depth = self.depth;
        sub.failed_instantiations = std::mem::take(&mut self.failed_instantiations);
        sub
    }

    pub fn parse(&mut self) -> Result<&'arena [Statement<'arena>], ParseError> {
        let statements = self.parse_block()?;
        if !self.is_at_end() {
            return Err(self.unexpected());
        }
        Ok(statements)
    }

    /// Runs `f` one nesting level deeper, failing past [`MAX_DEPTH`] and
    /// growing the stack on demand so that no depth below it can overflow.
    #[inline]
    pub(crate) fn nested<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        if self.depth >= MAX_DEPTH {
            return Err(ParseError::TooDeep {
                span: self.current_span(),
            });
        }
        self.depth += 1;
        let result = ensure_stack(|| f(self));
        self.depth -= 1;
        result
    }

    /// One or more `item`s separated by commas.
    pub(crate) fn comma_list<T>(
        &mut self,
        mut item: impl FnMut(&mut Self) -> Result<T, ParseError>,
    ) -> Result<Vec<T>, ParseError> {
        let mut items = vec![item(self)?];
        while self.consume(Type::Comma) {
            items.push(item(self)?);
        }
        Ok(items)
    }

    /// Moves `items` into the arena at exact size. Building lists in a heap
    /// `Vec` first keeps the arena free of the buffers a growing list leaves.
    #[inline]
    pub(crate) fn alloc_list<T>(&self, items: Vec<T>) -> &'arena [T] {
        self.arena.alloc_slice_fill_iter(items)
    }

    #[inline]
    pub(crate) fn peek(&self) -> Option<&'arena Token> {
        self.tokens.get(self.position)
    }

    #[inline]
    pub(crate) fn peek_type(&self) -> Option<Type> {
        self.peek().map(|t| t.token_type)
    }

    /// As [`Parser::peek_type`], but reads the rest of a `>>` whose first
    /// `>` closed a generic as `>`.
    #[inline]
    pub(crate) fn peek_operator(&self) -> Option<Type> {
        #[cfg(feature = "luau")]
        if self.pending_angle == Some(self.position) {
            return Some(Type::GreaterThan);
        }
        self.peek_type()
    }

    #[inline]
    pub(crate) fn peek_type_at(&self, offset: usize) -> Option<Type> {
        self.tokens
            .get(self.position + offset)
            .map(|t| t.token_type)
    }

    #[inline]
    pub(crate) fn advance(&mut self) -> Result<&'arena Token, ParseError> {
        let token = self
            .tokens
            .get(self.position)
            .ok_or(ParseError::EndOfInput {
                span: Span::new(self.eof, self.eof),
            })?;
        self.position += 1;
        Ok(token)
    }

    #[inline]
    pub(crate) fn is_at_end(&self) -> bool {
        self.position >= self.tokens.len()
    }

    #[inline]
    pub(crate) fn check(&self, token_type: Type) -> bool {
        self.peek_type() == Some(token_type)
    }

    pub(crate) fn expect(
        &mut self,
        token_type: Type,
        expected: &'static str,
    ) -> Result<&'arena Token, ParseError> {
        if self.check(token_type) {
            self.advance()
        } else {
            Err(self.expected(expected))
        }
    }

    #[inline]
    pub(crate) fn consume(&mut self, token_type: Type) -> bool {
        let hit = self.check(token_type);
        if hit {
            self.position += 1;
        }
        hit
    }

    /// The span of the current token, or an empty span at end of input.
    pub(crate) fn current_span(&self) -> Span {
        self.peek()
            .map_or(Span::new(self.eof, self.eof), |token| token.span)
    }

    pub(crate) fn current_description(&self) -> (String, Span) {
        match self.peek() {
            Some(token) => (token.token_type.to_string(), token.span),
            None => ("end of input".to_owned(), self.current_span()),
        }
    }

    pub(crate) fn expected(&self, expected: &'static str) -> ParseError {
        let (got, span) = self.current_description();
        ParseError::Expected {
            expected,
            got,
            span,
        }
    }

    pub(crate) fn unexpected(&self) -> ParseError {
        let (token, span) = self.current_description();
        ParseError::Unexpected { token, span }
    }

    #[inline]
    pub(crate) fn token_str(&self, token: &Token) -> &'source str {
        &self.source[token.span.range()]
    }

    #[inline]
    pub(crate) fn intern_token(&mut self, token: &Token) -> Symbol {
        let text = self.token_str(token);
        self.interner.intern(text)
    }

    /// The end of the last consumed token, or `fallback` before the first.
    #[inline]
    pub(crate) fn last_span_end(&self, fallback: u32) -> u32 {
        self.position
            .checked_sub(1)
            .and_then(|index| self.tokens.get(index))
            .map_or(fallback, |t| t.span.end)
    }

    /// The end of `annotation`, just parsed, including any grouping
    /// parentheses that its own span leaves out.
    #[cfg(feature = "luau")]
    pub(crate) fn type_end(&self, annotation: &TypeAnnotation<'_>) -> u32 {
        use crate::ast::Spanned;
        let end = annotation.span().end;
        end.max(self.last_span_end(end))
    }

    /// An optional `: type` annotation, in dialects with types.
    pub(crate) fn parse_optional_type(
        &mut self,
    ) -> Result<Option<TypeAnnotation<'arena>>, ParseError> {
        #[cfg(feature = "luau")]
        if self.dialect.has_types() && self.consume(Type::Colon) {
            return self.parse_type_annotation().map(Some);
        }
        Ok(None)
    }

    /// As [`Parser::parse_optional_type`], but the annotation may be a pack.
    pub(crate) fn parse_optional_type_or_pack(
        &mut self,
    ) -> Result<Option<TypeAnnotation<'arena>>, ParseError> {
        #[cfg(feature = "luau")]
        if self.dialect.has_types() && self.consume(Type::Colon) {
            return self.parse_type_or_pack().map(Some);
        }
        Ok(None)
    }

    /// Optional function return types: `: T` or `: (T, U)`.
    pub(crate) fn parse_optional_return_types(
        &mut self,
    ) -> Result<Option<&'arena [TypeAnnotation<'arena>]>, ParseError> {
        #[cfg(feature = "luau")]
        if self.dialect.has_types() && self.consume(Type::Colon) {
            return Ok(Some(self.parse_return_types()?));
        }
        Ok(None)
    }

    /// Consumes one `>` closing a generic, splitting a `>>` token when needed.
    /// Returns the end offset of the consumed `>`.
    #[cfg(feature = "luau")]
    pub(crate) fn expect_close_angle(&mut self) -> Result<u32, ParseError> {
        match self.peek() {
            Some(token) if token.token_type == Type::GreaterThan => {
                self.position += 1;
                Ok(token.span.end)
            }
            Some(token) if token.token_type == Type::ShiftRight => {
                if self.pending_angle == Some(self.position) {
                    self.pending_angle = None;
                    self.position += 1;
                    Ok(token.span.end)
                } else {
                    self.pending_angle = Some(self.position);
                    Ok(token.span.start + 1)
                }
            }
            _ => Err(self.expected("'>'")),
        }
    }

    #[cfg(feature = "luau")]
    pub(crate) fn check_close_angle(&self) -> bool {
        matches!(self.peek_type(), Some(Type::GreaterThan | Type::ShiftRight))
    }
}
