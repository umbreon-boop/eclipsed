//! Trivia (comments and whitespace) and the tokens that carry it.

use crate::lexer::{Span, Type};

/// A run of non-code source text between two tokens. Line endings are always
/// their own [`TriviaKind::Newline`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TriviaKind {
    /// Spaces, tabs, form feeds and vertical tabs.
    Whitespace,
    /// `\n`, `\r` or `\r\n`.
    Newline,
    /// `-- ...`, excluding the line ending.
    LineComment,
    /// `--[==[ ... ]==]`.
    LongComment,
    /// CfxLua `/* ... */`.
    CStyleComment,
    /// `#!...` on the first line.
    Shebang,
}

impl TriviaKind {
    #[inline]
    pub fn is_comment(self) -> bool {
        matches!(
            self,
            TriviaKind::LineComment | TriviaKind::LongComment | TriviaKind::CStyleComment
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Trivia {
    pub kind: TriviaKind,
    pub span: Span,
}

/// A token with the trivia attached to it.
///
/// Trivia after a token up to and including the next newline is its
/// `trailing`; the rest is the next token's `leading`. Both slices are sorted
/// and contiguous with the token, so concatenating `leading`, the token text
/// and `trailing` over every token reproduces the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenRef<'arena> {
    pub token_type: Type,
    pub span: Span,
    pub leading: &'arena [Trivia],
    pub trailing: &'arena [Trivia],
}

impl TokenRef<'_> {
    /// The token's own text, excluding trivia.
    #[inline]
    pub fn text<'source>(&self, source: &'source str) -> &'source str {
        &source[self.span.range()]
    }

    /// The span including attached trivia.
    pub fn full_span(&self) -> Span {
        let start = self
            .leading
            .first()
            .map_or(self.span.start, |t| t.span.start);
        let end = self.trailing.last().map_or(self.span.end, |t| t.span.end);
        Span::new(start, end)
    }
}
