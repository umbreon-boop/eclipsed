//! A single-call entry point that owns the arena, interner, lexer and parser.
//!
//! ```no_run
//! use eclipsed::{Dialect, api};
//!
//! let source = std::fs::read_to_string("init.lua").unwrap();
//! let comments = api::with_chunk(&source, Dialect::Lua, |chunk| chunk.comments().len());
//! ```

use bumpalo::Bump;

use crate::ast::Statement;
use crate::cst::{TokenRef, Trivia};
use crate::dialect::Dialect;
use crate::interner::{Interner, Symbol};
use crate::lexer::{Lexer, Span};
use crate::parser::{ParseError, Parser};

/// A parsed chunk together with its tokens and interned names.
#[derive(Debug)]
pub struct Chunk<'a> {
    source: &'a str,
    dialect: Dialect,
    statements: &'a [Statement<'a>],
    tokens: &'a [TokenRef<'a>],
    interner: &'a Interner<'a>,
}

impl<'a> Chunk<'a> {
    pub fn source(&self) -> &'a str {
        self.source
    }

    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    pub fn statements(&self) -> &'a [Statement<'a>] {
        self.statements
    }

    /// Every token in source order, ending with [`crate::lexer::Type::EndOfFile`].
    pub fn tokens(&self) -> &'a [TokenRef<'a>] {
        self.tokens
    }

    pub fn interner(&self) -> &'a Interner<'a> {
        self.interner
    }

    /// The text of a symbol stored in this chunk's tree.
    pub fn resolve(&self, symbol: Symbol) -> &'a str {
        self.interner.resolve(symbol)
    }

    /// The source text covered by `span`.
    ///
    /// # Panics
    ///
    /// Panics if `span` is out of bounds or not on character boundaries.
    pub fn text(&self, span: Span) -> &'a str {
        &self.source[span.range()]
    }

    /// Every comment in source order, as `(span, text)` with delimiters.
    pub fn comments(&self) -> Vec<(Span, &'a str)> {
        self.tokens
            .iter()
            .flat_map(|token| token.leading.iter().chain(token.trailing))
            .filter(|trivia| trivia.kind.is_comment())
            .map(|trivia| (trivia.span, self.text(trivia.span)))
            .collect()
    }

    /// The comments leading the token that starts at `span.start`, verbatim.
    ///
    /// A comment trailing the previous line belongs to that line's last token
    /// and is not included.
    pub fn doc_comments_for(&self, span: Span) -> Vec<&'a str> {
        self.doc_comment_trivia(span)
            .map(|trivia| self.text(trivia.span))
            .collect()
    }

    /// As [`Chunk::doc_comments_for`], but as spans.
    pub fn doc_comment_spans_for(&self, span: Span) -> Vec<Span> {
        self.doc_comment_trivia(span)
            .map(|trivia| trivia.span)
            .collect()
    }

    fn doc_comment_trivia(&self, span: Span) -> impl Iterator<Item = &'a Trivia> {
        let index = self
            .tokens
            .partition_point(|token| token.span.start < span.start);
        self.tokens
            .get(index)
            .filter(|token| token.span.start == span.start)
            .into_iter()
            .flat_map(|token| token.leading)
            .filter(|trivia| trivia.kind.is_comment())
    }
}

/// Parses `source` and passes the resulting [`Chunk`] to `f`.
///
/// The chunk borrows an arena local to this call, so `f` must extract
/// whatever it needs before returning.
///
/// # Panics
///
/// Panics if `source` is longer than `u32::MAX` bytes.
pub fn with_chunk<R>(
    source: &str,
    dialect: Dialect,
    f: impl FnOnce(&Chunk<'_>) -> R,
) -> Result<R, ParseError> {
    let arena = Bump::new();
    let tokens = Lexer::new(source, &arena, dialect).lex_cst();
    let mut interner = Interner::new();
    let statements = Parser::new_cst(tokens, source, &arena, &mut interner, dialect).parse()?;
    Ok(f(&Chunk {
        source,
        dialect,
        statements,
        tokens,
        interner: &interner,
    }))
}
