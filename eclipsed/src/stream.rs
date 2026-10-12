//! A flat, owned token stream in which trivia are ordinary tokens.
//!
//! Printing is concatenation, so an unmodified stream renders back to its
//! source. Passes rebuild the stream through [`TokenStream::rewrite`], which
//! reuses one scratch buffer rather than splicing.

use std::borrow::Cow;
use std::fmt;

use crate::Dialect;
use crate::cst::{TokenRef, Trivia, TriviaKind};
use crate::lexer::{Lexer, Span, Type};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    Syntax(Type),
    Trivia(TriviaKind),
}

impl TokenKind {
    #[inline]
    pub fn is_trivia(self) -> bool {
        matches!(self, TokenKind::Trivia(_))
    }

    #[inline]
    pub fn is_comment(self) -> bool {
        matches!(self, TokenKind::Trivia(kind) if kind.is_comment())
    }

    #[inline]
    pub fn is_newline(self) -> bool {
        matches!(self, TokenKind::Trivia(TriviaKind::Newline))
    }
}

/// A token and the text it prints as.
///
/// `span` records where a lexed token came from and is `None` for one a pass
/// created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token<'src> {
    pub kind: TokenKind,
    pub span: Option<Span>,
    pub text: Cow<'src, str>,
}

impl<'src> Token<'src> {
    /// A token created by a pass rather than read from the source.
    pub fn new(kind: TokenKind, text: impl Into<Cow<'src, str>>) -> Self {
        Token {
            kind,
            span: None,
            text: text.into(),
        }
    }

    pub fn space() -> Self {
        Token::new(TokenKind::Trivia(TriviaKind::Whitespace), " ")
    }

    pub fn newline() -> Self {
        Token::new(TokenKind::Trivia(TriviaKind::Newline), "\n")
    }

    /// Whether this token was read from the source.
    pub fn is_original(&self) -> bool {
        self.span.is_some()
    }
}

#[derive(Debug, Clone, Default)]
pub struct TokenStream<'src> {
    tokens: Vec<Token<'src>>,
    scratch: Vec<Token<'src>>,
}

impl<'src> TokenStream<'src> {
    /// Lexes `source`; every token borrows `source`.
    ///
    /// # Panics
    ///
    /// Panics if `source` is longer than `u32::MAX` bytes.
    pub fn lex(source: &'src str, dialect: Dialect) -> Self {
        let arena = bumpalo::Bump::new();
        let cst = Lexer::new(source, &arena, dialect).lex_cst();

        let mut tokens = Vec::with_capacity(cst.len() * 2);
        tokens.extend(flatten(cst).map(|(kind, span)| Token {
            kind,
            span: Some(span),
            text: Cow::Borrowed(&source[span.range()]),
        }));

        TokenStream {
            scratch: Vec::with_capacity(tokens.len()),
            tokens,
        }
    }

    pub fn tokens(&self) -> &[Token<'src>] {
        &self.tokens
    }

    pub fn tokens_mut(&mut self) -> &mut Vec<Token<'src>> {
        &mut self.tokens
    }

    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// Runs one pass: `pass` reads the current tokens and appends the new
    /// stream to the output buffer.
    ///
    /// ```
    /// # use eclipsed::{Dialect, stream::TokenStream};
    /// let mut stream = TokenStream::lex("a = 1 -- note", Dialect::Lua);
    /// stream.rewrite(|input, out| {
    ///     out.extend(input.iter().filter(|t| !t.kind.is_comment()).cloned());
    /// });
    /// assert_eq!(stream.render(), "a = 1 ");
    /// ```
    pub fn rewrite<F>(&mut self, pass: F)
    where
        F: FnOnce(&[Token<'src>], &mut Vec<Token<'src>>),
    {
        self.scratch.clear();
        pass(&self.tokens, &mut self.scratch);
        std::mem::swap(&mut self.tokens, &mut self.scratch);
    }

    pub fn render(&self) -> String {
        let mut out = String::with_capacity(self.tokens.iter().map(|t| t.text.len()).sum());
        for token in &self.tokens {
            out.push_str(&token.text);
        }
        out
    }
}

impl fmt::Display for TokenStream<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.tokens
            .iter()
            .try_for_each(|token| f.write_str(&token.text))
    }
}

impl<'src> IntoIterator for TokenStream<'src> {
    type Item = Token<'src>;
    type IntoIter = std::vec::IntoIter<Token<'src>>;

    fn into_iter(self) -> Self::IntoIter {
        self.tokens.into_iter()
    }
}

impl<'src> FromIterator<Token<'src>> for TokenStream<'src> {
    fn from_iter<I: IntoIterator<Item = Token<'src>>>(iter: I) -> Self {
        TokenStream {
            tokens: iter.into_iter().collect(),
            scratch: Vec::new(),
        }
    }
}

/// Every token and trivia in `tokens`, in source order, without the closing
/// [`Type::EndOfFile`].
pub fn flatten<'a>(tokens: &'a [TokenRef<'a>]) -> impl Iterator<Item = (TokenKind, Span)> + 'a {
    let trivia = |trivia: &Trivia| (TokenKind::Trivia(trivia.kind), trivia.span);
    tokens.iter().flat_map(move |token| {
        let syntax = (token.token_type != Type::EndOfFile)
            .then_some((TokenKind::Syntax(token.token_type), token.span));
        token
            .leading
            .iter()
            .map(trivia)
            .chain(syntax)
            .chain(token.trailing.iter().map(trivia))
    })
}

/// Whether `a` and `b` would lex differently if written with nothing between
/// them, as `local` `x` or `-` `-x` would.
pub fn would_fuse(a: &str, b: &str, dialect: Dialect) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    quick_fuse(a.as_bytes(), b.as_bytes()).unwrap_or_else(|| lexes_together(a, b, dialect))
}

/// Characters that never begin or continue a longer token.
fn is_isolated(byte: u8) -> bool {
    matches!(byte, b'(' | b')' | b'{' | b'}' | b']' | b',' | b';')
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Answers the common cases without lexing, or `None` when unsure.
fn quick_fuse(a: &[u8], b: &[u8]) -> Option<bool> {
    let (first, next) = (a[0], b[0]);
    let a_is_word = (first.is_ascii_alphabetic() || first == b'_') && a.iter().all(|&c| is_word(c));
    if a_is_word && is_word(next) {
        return Some(true);
    }
    if a_is_word && next.is_ascii() && !is_word(next) {
        return Some(false);
    }
    if a.len() == 1 && is_isolated(first) {
        return Some(false);
    }
    if b.len() == 1
        && is_isolated(next)
        && (a_is_word || first.is_ascii_digit() || b"+-*/%^#=<>~:.&|?!@".contains(&first))
    {
        return Some(false);
    }
    None
}

fn lexes_together(a: &str, b: &str, dialect: Dialect) -> bool {
    let joined = [a, b].concat();
    let arena = bumpalo::Bump::new();
    match Lexer::new(&joined, &arena, dialect).next_token() {
        Some(token) => token.span.end as usize != a.len(),
        // Only trivia: the pair formed a comment.
        None => true,
    }
}

/// Drops all trivia, inserting a space only where two tokens would fuse.
pub fn strip_trivia(stream: &mut TokenStream<'_>, dialect: Dialect) {
    stream.rewrite(|input, out| {
        for token in input.iter().filter(|token| !token.kind.is_trivia()) {
            if out.last().is_some_and(|previous: &Token<'_>| {
                would_fuse(&previous.text, &token.text, dialect)
            }) {
                out.push(Token::space());
            }
            out.push(token.clone());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_fuse_agrees_with_the_lexer() {
        let spellings = [
            "x",
            "_a",
            "end",
            "local",
            "and",
            "a1",
            "\u{e9}t\u{e9}",
            "1",
            "1.",
            ".5",
            "0x1F",
            "1e5",
            "1LL",
            "1i",
            "0b101",
            "1_000",
            "\"s\"",
            "'s'",
            "[[s]]",
            "[==[s]==]",
            "`s`",
            "`{x}`",
            "\"x",
            "'x",
            "[",
            "[=",
            "]",
            "(",
            ")",
            "{",
            "}",
            ",",
            ";",
            ":",
            "::",
            ".",
            "..",
            "...",
            "..=",
            "=",
            "==",
            "~=",
            "~",
            "<",
            "<=",
            "<<",
            "<<=",
            ">",
            ">=",
            ">>",
            ">>=",
            "+",
            "+=",
            "-",
            "-=",
            "->",
            "*",
            "*=",
            "/",
            "/=",
            "//",
            "//=",
            "%",
            "%=",
            "^",
            "^=",
            "#",
            "&",
            "&=",
            "|",
            "|=",
            "?",
            "!",
            "@",
            "!=",
            "-x",
            "/x",
            "=x",
            "*/",
        ];
        let single = |text: &str, dialect| {
            let arena = bumpalo::Bump::new();
            let tokens = Lexer::new(text, &arena, dialect).lex();
            tokens.len() == 1 && tokens[0].span.end as usize == text.len()
        };
        for &dialect in Dialect::ALL {
            let tokens: Vec<&str> = spellings
                .iter()
                .copied()
                .filter(|t| single(t, dialect))
                .collect();
            for &a in &tokens {
                for &b in &tokens {
                    if let Some(quick) = quick_fuse(a.as_bytes(), b.as_bytes()) {
                        assert_eq!(
                            quick,
                            lexes_together(a, b, dialect),
                            "{a:?} {b:?} in {dialect}"
                        );
                    }
                }
            }
        }
    }
}
