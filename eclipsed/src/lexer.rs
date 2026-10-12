//! Byte-oriented lexer producing either plain tokens or, with the `cst`
//! feature, tokens carrying their surrounding trivia.

use std::fmt;
use std::ops::Range;

use bumpalo::Bump;

use crate::dialect::Dialect;

macro_rules! token_types {
    ($($(#[doc = $doc:expr])* $(#[cfg($cfg:meta)])? $variant:ident => $display:literal,)*) => {
        /// The kind of a lexed token.
        ///
        /// Discriminants depend on the enabled feature set; use [`Type::name`]
        /// when a stable identifier is needed.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Type {
            $($(#[doc = $doc])* $(#[cfg($cfg)])? $variant,)*
        }

        impl Type {
            /// Every variant, indexed by its discriminant.
            pub const ALL: &'static [Type] = &[$($(#[cfg($cfg)])? Type::$variant,)*];

            /// The variant's identifier, e.g. `"DoubleColon"`.
            pub const fn name(self) -> &'static str {
                match self {
                    $($(#[cfg($cfg)])? Type::$variant => stringify!($variant),)*
                }
            }

            const fn description(self) -> &'static str {
                match self {
                    $($(#[cfg($cfg)])? Type::$variant => $display,)*
                }
            }
        }
    };
}

token_types! {
    Identifier => "identifier",
    Integer => "integer",
    Float => "float",
    String => "string",
    LongString => "long string",

    And => "'and'",
    Break => "'break'",
    Do => "'do'",
    Else => "'else'",
    Elseif => "'elseif'",
    End => "'end'",
    False => "'false'",
    For => "'for'",
    Function => "'function'",
    Goto => "'goto'",
    If => "'if'",
    In => "'in'",
    Local => "'local'",
    Nil => "'nil'",
    Not => "'not'",
    Or => "'or'",
    Repeat => "'repeat'",
    Return => "'return'",
    Then => "'then'",
    True => "'true'",
    Until => "'until'",
    While => "'while'",

    Plus => "'+'",
    Minus => "'-'",
    Star => "'*'",
    Slash => "'/'",
    Percent => "'%'",
    Caret => "'^'",
    Hash => "'#'",
    Ampersand => "'&'",
    Tilde => "'~'",
    Pipe => "'|'",
    ShiftLeft => "'<<'",
    ShiftRight => "'>>'",
    DoubleSlash => "'//'",
    Equal => "'=='",
    NotEqual => "'~='",
    LessThan => "'<'",
    GreaterThan => "'>'",
    LessThanOrEqual => "'<='",
    GreaterThanOrEqual => "'>='",
    Assign => "'='",
    LeftParenthesis => "'('",
    RightParenthesis => "')'",
    LeftCurlyBrace => "'{'",
    RightCurlyBrace => "'}'",
    LeftSquareBracket => "'['",
    RightSquareBracket => "']'",
    DoubleColon => "'::'",
    Semicolon => "';'",
    Colon => "':'",
    Comma => "','",
    Dot => "'.'",
    DoubleDot => "'..'",
    TripleDot => "'...'",
    Arrow => "'->'",
    QuestionMark => "'?'",
    ExclamationMark => "'!'",

    #[cfg(feature = "compound")]
    PlusAssign => "'+='",
    #[cfg(feature = "compound")]
    MinusAssign => "'-='",
    #[cfg(feature = "compound")]
    StarAssign => "'*='",
    #[cfg(feature = "compound")]
    SlashAssign => "'/='",
    #[cfg(feature = "compound")]
    DoubleSlashAssign => "'//='",
    #[cfg(feature = "compound")]
    PercentAssign => "'%='",
    #[cfg(feature = "compound")]
    CaretAssign => "'^='",
    #[cfg(feature = "compound")]
    DoubleDotAssign => "'..='",
    #[cfg(feature = "cfxlua")]
    AmpersandAssign => "'&='",
    #[cfg(feature = "cfxlua")]
    PipeAssign => "'|='",
    #[cfg(feature = "cfxlua")]
    ShiftLeftAssign => "'<<='",
    #[cfg(feature = "cfxlua")]
    ShiftRightAssign => "'>>='",
    /// CfxLua compile-time jenkins hash string (`` `text` ``).
    #[cfg(feature = "cfxlua")]
    HashString => "hash string",
    /// Luau interpolated string, spanning the whole `` `...` `` literal.
    #[cfg(feature = "luau")]
    InterpolatedString => "interpolated string",
    /// Luau attribute marker (`@`).
    #[cfg(feature = "luau")]
    At => "'@'",

    /// Zero-width token closing every [`Lexer::lex_cst`] stream.
    #[cfg(feature = "cst")]
    EndOfFile => "end of file",

    UnterminatedString => "unterminated string",
    UnterminatedLongString => "unterminated long string",
    MalformedNumber => "malformed number",
    UnexpectedByte => "unexpected byte",
}

impl Type {
    /// Whether this is one of the lexer's error tokens.
    pub const fn is_error(self) -> bool {
        matches!(
            self,
            Type::UnterminatedString
                | Type::UnterminatedLongString
                | Type::MalformedNumber
                | Type::UnexpectedByte
        )
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.description())
    }
}

/// A half-open byte range `[start, end)` into the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    #[inline]
    pub const fn new(start: u32, end: u32) -> Self {
        Span { start, end }
    }

    #[inline]
    pub const fn len(self) -> u32 {
        self.end - self.start
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// The smallest span covering both `self` and `other`.
    #[inline]
    pub fn to(self, other: Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }

    #[inline]
    pub const fn contains(self, offset: u32) -> bool {
        self.start <= offset && offset < self.end
    }

    #[inline]
    pub fn range(self) -> Range<usize> {
        self.start as usize..self.end as usize
    }
}

impl From<Span> for Range<usize> {
    fn from(span: Span) -> Self {
        span.range()
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub token_type: Type,
    pub span: Span,
}

/// A comment successfully scanned by [`Lexer::lex_comment`].
#[derive(Clone, Copy)]
enum Comment {
    Line,
    Long,
    #[cfg(feature = "cfxlua")]
    CStyle,
}

/// Lua's `lisspace`.
#[inline]
fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

#[derive(Debug)]
pub struct Lexer<'source, 'arena> {
    source: &'source [u8],
    length: u32,
    position: u32,
    current: u8,
    dialect: Dialect,
    arena: &'arena Bump,
    /// Trivia since the last token, reused across tokens.
    #[cfg(feature = "cst")]
    pending_trivia: Vec<crate::cst::Trivia>,
}

impl<'source, 'arena> Lexer<'source, 'arena> {
    /// # Panics
    ///
    /// Panics if `source` is longer than `u32::MAX` bytes.
    pub fn new(source: &'source str, arena: &'arena Bump, dialect: Dialect) -> Self {
        let bytes = source.as_bytes();
        let length = u32::try_from(bytes.len()).expect("source exceeds u32::MAX bytes");
        let mut lexer = Self::over(bytes, arena, dialect, 0, length);

        if bytes.starts_with(b"#!") {
            while !lexer.at_end() && !matches!(lexer.current, b'\n' | b'\r') {
                lexer.advance();
            }
        }
        lexer
    }

    fn over(
        source: &'source [u8],
        arena: &'arena Bump,
        dialect: Dialect,
        start: u32,
        end: u32,
    ) -> Self {
        let end = end.min(source.len() as u32);
        let start = start.min(end);
        Self {
            source,
            length: end,
            position: start,
            current: if start < end {
                source[start as usize]
            } else {
                0
            },
            dialect,
            arena,
            #[cfg(feature = "cst")]
            pending_trivia: Vec::new(),
        }
    }

    /// Lexes `source[start..end]`, producing spans relative to all of `source`.
    #[cfg(feature = "luau")]
    pub fn lex_range(
        source: &'source str,
        arena: &'arena Bump,
        dialect: Dialect,
        start: u32,
        end: u32,
    ) -> &'arena [Token] {
        Self::over(source.as_bytes(), arena, dialect, start, end).lex()
    }

    /// Lexes the interpolation hole starting at `start`, just after its `{`,
    /// returning its tokens and the offset of its closing `}`, or `end` if it
    /// is unclosed.
    #[cfg(feature = "luau")]
    pub fn lex_hole(
        source: &'source str,
        arena: &'arena Bump,
        dialect: Dialect,
        start: u32,
        end: u32,
    ) -> (&'arena [Token], u32) {
        let mut lexer = Self::over(source.as_bytes(), arena, dialect, start, end);
        let mut tokens = Vec::new();
        let mut depth = 0u32;
        while let Some(token) = lexer.next_token() {
            match token.token_type {
                Type::LeftCurlyBrace => depth += 1,
                Type::RightCurlyBrace if depth == 0 => {
                    return (arena.alloc_slice_copy(&tokens), token.span.start);
                }
                Type::RightCurlyBrace => depth -= 1,
                _ => {}
            }
            tokens.push(token);
        }
        (arena.alloc_slice_copy(&tokens), lexer.length)
    }

    pub fn lex(mut self) -> &'arena [Token] {
        let mut tokens = Vec::with_capacity(self.estimated_tokens());
        while let Some(token) = self.next_token() {
            tokens.push(token);
        }
        self.arena.alloc_slice_copy(&tokens)
    }

    /// Scans the next token, or returns `None` once only trivia remains.
    pub fn next_token(&mut self) -> Option<Token> {
        loop {
            while is_space(self.current) {
                self.advance();
            }
            if self.at_end() {
                return None;
            }
            let start = self.position;
            match self.lex_comment() {
                Some(Ok(_)) => continue,
                Some(Err(error)) => return Some(self.create_token(error, start)),
                None => return Some(self.scan_token()),
            }
        }
    }

    /// Scans a comment at the cursor. `None` means there is none; `Err` holds
    /// the error token for an unterminated one.
    fn lex_comment(&mut self) -> Option<Result<Comment, Type>> {
        #[cfg(feature = "cfxlua")]
        if self.dialect.has_c_style_comments() && self.current == b'/' && self.lookahead(1) == b'*'
        {
            self.advance();
            self.advance();
            while !self.at_end() {
                if self.advance() == b'*' && self.current == b'/' {
                    self.advance();
                    return Some(Ok(Comment::CStyle));
                }
            }
            return Some(Err(Type::UnterminatedLongString));
        }

        if self.current != b'-' || self.lookahead(1) != b'-' {
            return None;
        }
        self.advance();
        self.advance();

        if self.current == b'['
            && let Some(level) = self.long_bracket_level(1)
        {
            for _ in 0..level + 2 {
                self.advance();
            }
            return Some(if self.skip_long_body(level) {
                Ok(Comment::Long)
            } else {
                Err(Type::UnterminatedLongString)
            });
        }

        while !self.at_end() && !matches!(self.current, b'\n' | b'\r') {
            self.advance();
        }
        Some(Ok(Comment::Line))
    }

    fn scan_token(&mut self) -> Token {
        let start = self.position;
        let byte = self.advance();

        let token = match byte {
            b'+' => self.compound_or(Type::Plus),
            b'*' => self.compound_or(Type::Star),
            b'%' => self.compound_or(Type::Percent),
            b'^' => self.compound_or(Type::Caret),
            b'&' => self.bitwise_compound_or(Type::Ampersand),
            b'|' => self.bitwise_compound_or(Type::Pipe),
            b'(' => Type::LeftParenthesis,
            b')' => Type::RightParenthesis,
            b'{' => Type::LeftCurlyBrace,
            b'}' => Type::RightCurlyBrace,
            b']' => Type::RightSquareBracket,
            b';' => Type::Semicolon,
            b',' => Type::Comma,
            b'#' => Type::Hash,
            b'?' => Type::QuestionMark,
            b'!' => Type::ExclamationMark,
            b'-' if self.eat(b'>') => Type::Arrow,
            b'-' => self.compound_or(Type::Minus),
            b'/' if self.eat(b'/') => self.compound_or(Type::DoubleSlash),
            b'/' => self.compound_or(Type::Slash),
            b'~' if self.eat(b'=') => Type::NotEqual,
            b'~' => Type::Tilde,
            b'=' if self.eat(b'=') => Type::Equal,
            b'=' => Type::Assign,
            b'<' if self.eat(b'=') => Type::LessThanOrEqual,
            b'<' if self.eat(b'<') => self.bitwise_compound_or(Type::ShiftLeft),
            b'<' => Type::LessThan,
            b'>' if self.eat(b'=') => Type::GreaterThanOrEqual,
            b'>' if self.eat(b'>') => self.bitwise_compound_or(Type::ShiftRight),
            b'>' => Type::GreaterThan,
            b':' if self.eat(b':') => Type::DoubleColon,
            b':' => Type::Colon,
            b'.' if self.eat(b'.') => {
                if self.eat(b'.') {
                    Type::TripleDot
                } else {
                    self.compound_or(Type::DoubleDot)
                }
            }
            b'.' if self.current.is_ascii_digit() => self.lex_number(start),
            b'.' => Type::Dot,
            b'[' => match self.long_bracket_level(0) {
                Some(level) => {
                    for _ in 0..=level {
                        self.advance();
                    }
                    if self.skip_long_body(level) {
                        Type::LongString
                    } else {
                        Type::UnterminatedLongString
                    }
                }
                None => Type::LeftSquareBracket,
            },
            b'"' | b'\'' => self.lex_string(byte),
            #[cfg(feature = "luau")]
            b'`' if self.dialect.has_string_interpolation() => self.lex_interpolated_string(),
            #[cfg(feature = "cfxlua")]
            b'`' if self.dialect.has_jenkins_hash_strings() => match self.lex_string(b'`') {
                Type::String => Type::HashString,
                other => other,
            },
            #[cfg(feature = "luau")]
            b'@' if self.dialect.has_attributes() => Type::At,
            b'0'..=b'9' => self.lex_number(start),
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.lex_word(start),
            0x80.. if self.dialect.has_extended_identifiers() => self.lex_word(start),
            _ => {
                // Keep the span on a character boundary.
                while self.current & 0xC0 == 0x80 && !self.at_end() {
                    self.advance();
                }
                Type::UnexpectedByte
            }
        };

        self.create_token(token, start)
    }

    /// Consumes the whitespace run at the cursor into `pending_trivia`, one
    /// `Newline` per line ending so no `Whitespace` trivia spans lines.
    #[cfg(feature = "cst")]
    fn collect_whitespace(&mut self) {
        use crate::cst::TriviaKind;

        loop {
            let start = self.position;
            match self.current {
                b' ' | b'\t' | 0x0b | 0x0c => {
                    while matches!(self.current, b' ' | b'\t' | 0x0b | 0x0c) {
                        self.advance();
                    }
                    self.push_trivia(TriviaKind::Whitespace, start);
                }
                b'\n' => {
                    self.advance();
                    self.push_trivia(TriviaKind::Newline, start);
                }
                b'\r' => {
                    self.advance();
                    self.eat(b'\n');
                    self.push_trivia(TriviaKind::Newline, start);
                }
                _ => return,
            }
        }
    }

    #[cfg(feature = "cst")]
    #[inline]
    fn push_trivia(&mut self, kind: crate::cst::TriviaKind, start: u32) {
        self.pending_trivia.push(crate::cst::Trivia {
            kind,
            span: Span::new(start, self.position),
        });
    }

    #[cfg(feature = "cst")]
    fn next_token_cst(&mut self) -> Option<Token> {
        use crate::cst::TriviaKind;

        loop {
            self.collect_whitespace();
            if self.at_end() {
                return None;
            }
            let start = self.position;
            let kind = match self.lex_comment() {
                Some(Ok(Comment::Line)) => TriviaKind::LineComment,
                Some(Ok(Comment::Long)) => TriviaKind::LongComment,
                #[cfg(feature = "cfxlua")]
                Some(Ok(Comment::CStyle)) => TriviaKind::CStyleComment,
                Some(Err(error)) => return Some(self.create_token(error, start)),
                None => return Some(self.scan_token()),
            };
            self.push_trivia(kind, start);
        }
    }

    /// Lexes into tokens that carry their comments and whitespace.
    ///
    /// The result always ends with a [`Type::EndOfFile`] token owning any
    /// trivia after the last real token; concatenating every token's leading
    /// trivia, text and trailing trivia reproduces the source exactly.
    #[cfg(feature = "cst")]
    pub fn lex_cst(mut self) -> &'arena [crate::cst::TokenRef<'arena>] {
        use crate::cst::{TokenRef, TriviaKind};

        let arena = self.arena;
        let mut out: Vec<TokenRef<'arena>> = Vec::with_capacity(self.estimated_tokens());

        if self.position > 0 {
            self.push_trivia(TriviaKind::Shebang, 0);
        }

        loop {
            let token = self.next_token_cst();
            let collected = &*arena.alloc_slice_copy(&self.pending_trivia);
            self.pending_trivia.clear();

            // Trivia up to and including the first newline trails the previous
            // token; the rest leads the next one.
            let leading = match out.last_mut() {
                Some(previous) => {
                    let split = collected
                        .iter()
                        .position(|t| t.kind == TriviaKind::Newline)
                        .map_or(collected.len(), |i| i + 1);
                    let (trailing, leading) = collected.split_at(split);
                    previous.trailing = trailing;
                    leading
                }
                None => collected,
            };

            let done = token.is_none();
            let token = token.unwrap_or(Token {
                token_type: Type::EndOfFile,
                span: Span::new(self.length, self.length),
            });
            out.push(TokenRef {
                token_type: token.token_type,
                span: token.span,
                leading,
                trailing: &[],
            });
            if done {
                return arena.alloc_slice_copy(&out);
            }
        }
    }

    /// Typical code has about one token per five bytes; the scratch vector
    /// this sizes is freed once its contents are copied into the arena.
    fn estimated_tokens(&self) -> usize {
        (self.length - self.position) as usize / 4
    }

    #[inline(always)]
    fn at_end(&self) -> bool {
        self.position >= self.length
    }

    #[inline(always)]
    fn advance(&mut self) -> u8 {
        let previous = self.current;
        self.position += 1;
        self.current = if self.position < self.length {
            self.source[self.position as usize]
        } else {
            0
        };
        previous
    }

    #[inline(always)]
    fn eat(&mut self, byte: u8) -> bool {
        let hit = self.current == byte && !self.at_end();
        if hit {
            self.advance();
        }
        hit
    }

    #[inline(always)]
    fn lookahead(&self, offset: u32) -> u8 {
        let target = self.position + offset;
        if target < self.length {
            self.source[target as usize]
        } else {
            0
        }
    }

    /// If `[` `=`* `[` starts at `offset` past the cursor, the number of `=`.
    fn long_bracket_level(&self, offset: u32) -> Option<u32> {
        let mut level = 0;
        while self.lookahead(offset + level) == b'=' {
            level += 1;
        }
        (self.lookahead(offset + level) == b'[').then_some(level)
    }

    /// Skips a long-bracket body up to and including its closing bracket.
    fn skip_long_body(&mut self, level: u32) -> bool {
        while !self.at_end() {
            if self.advance() == b']' {
                let mut equals = 0;
                while self.eat(b'=') {
                    equals += 1;
                }
                if equals == level && self.eat(b']') {
                    return true;
                }
            }
        }
        false
    }

    /// Consumes the escape sequence following a `\` that was just consumed.
    fn skip_escape(&mut self) -> bool {
        if self.at_end() {
            return false;
        }
        match self.advance() {
            b'z' => {
                while is_space(self.current) {
                    self.advance();
                }
            }
            b'u' if self.eat(b'{') => {
                while self.current.is_ascii_hexdigit() {
                    self.advance();
                }
                self.eat(b'}');
            }
            b'\r' => {
                self.eat(b'\n');
            }
            b'\n' => {
                self.eat(b'\r');
            }
            _ => {}
        }
        true
    }

    fn lex_string(&mut self, quote: u8) -> Type {
        loop {
            if self.at_end() || matches!(self.current, b'\n' | b'\r') {
                return Type::UnterminatedString;
            }
            match self.advance() {
                b'\\' if !self.skip_escape() => return Type::UnterminatedString,
                byte if byte == quote => return Type::String,
                _ => {}
            }
        }
    }

    #[inline]
    fn compound_or(&mut self, base: Type) -> Type {
        #[cfg(feature = "compound")]
        if self.dialect.has_compound_assignment()
            && self.current == b'='
            && let Some(compound) = compound_token(base)
        {
            self.advance();
            return compound;
        }
        base
    }

    #[inline]
    fn bitwise_compound_or(&mut self, base: Type) -> Type {
        #[cfg(feature = "cfxlua")]
        if self.dialect.has_bitwise_compound_assignment() && self.current == b'=' {
            let compound = match base {
                Type::Ampersand => Type::AmpersandAssign,
                Type::Pipe => Type::PipeAssign,
                Type::ShiftLeft => Type::ShiftLeftAssign,
                Type::ShiftRight => Type::ShiftRightAssign,
                _ => return base,
            };
            self.advance();
            return compound;
        }
        base
    }

    /// Lexes a Luau interpolated string after its opening backtick as a single
    /// token; the parser splits it into parts.
    #[cfg(feature = "luau")]
    fn lex_interpolated_string(&mut self) -> Type {
        // The brace depth of each open hole, innermost last. Holes and the
        // strings within them alternate, so nesting needs no recursion.
        let mut holes: Vec<u32> = Vec::new();
        let mut in_string = true;
        loop {
            if in_string {
                if self.at_end() || matches!(self.current, b'\n' | b'\r') {
                    return Type::UnterminatedString;
                }
                match self.advance() {
                    b'\\' if !self.skip_escape() => return Type::UnterminatedString,
                    b'`' if holes.is_empty() => return Type::InterpolatedString,
                    b'`' => in_string = false,
                    b'{' => {
                        holes.push(0);
                        in_string = false;
                    }
                    _ => {}
                }
                continue;
            }
            while is_space(self.current) {
                self.advance();
            }
            if self.at_end() {
                return Type::UnterminatedString;
            }
            if self.eat(b'`') {
                in_string = true;
                continue;
            }
            match self.lex_comment() {
                Some(Ok(_)) => continue,
                Some(Err(_)) => return Type::UnterminatedString,
                None => {}
            }
            let innermost = holes.len() - 1;
            match self.scan_token().token_type {
                Type::LeftCurlyBrace => holes[innermost] += 1,
                Type::RightCurlyBrace if holes[innermost] == 0 => {
                    holes.pop();
                    in_string = true;
                }
                Type::RightCurlyBrace => holes[innermost] -= 1,
                _ => {}
            }
        }
    }

    /// Rejects a numeral that runs straight into a word: `0and` is malformed,
    /// as in PUC Lua, not `0` followed by `and`.
    #[inline]
    fn finish_number(&mut self, kind: Type) -> Type {
        if !is_word_byte(self.current) {
            return kind;
        }
        while is_word_byte(self.current) {
            self.advance();
        }
        Type::MalformedNumber
    }

    /// Skips digits and separators, returning whether there was a digit.
    fn skip_digits(&mut self, digit: fn(&u8) -> bool) -> bool {
        let separators = self.dialect.has_digit_separators();
        let mut any = false;
        loop {
            if digit(&self.current) {
                any = true;
            } else if !(separators && self.current == b'_') {
                return any;
            }
            self.advance();
        }
    }

    fn lex_number(&mut self, start: u32) -> Type {
        let first = self.source[start as usize];
        if first == b'0' && matches!(self.current, b'x' | b'X' | b'b' | b'B') {
            let hex = matches!(self.advance(), b'x' | b'X');
            let digit: fn(&u8) -> bool = if hex {
                u8::is_ascii_hexdigit
            } else {
                |byte| matches!(byte, b'0' | b'1')
            };
            let mut has_digits = self.skip_digits(digit);
            let mut is_float = false;
            if hex && self.eat(b'.') {
                has_digits |= self.skip_digits(digit);
                is_float = true;
            }
            if hex && matches!(self.current, b'p' | b'P') {
                if !self.lex_exponent() {
                    return Type::MalformedNumber;
                }
                is_float = true;
            }
            self.consume_cdata_suffix();
            let kind = self.finish_number(if is_float { Type::Float } else { Type::Integer });
            return if has_digits {
                kind
            } else {
                Type::MalformedNumber
            };
        }

        let mut is_float = first == b'.';
        self.skip_digits(u8::is_ascii_digit);
        if !is_float && self.eat(b'.') {
            is_float = true;
            self.skip_digits(u8::is_ascii_digit);
        }
        if matches!(self.current, b'e' | b'E') {
            if !self.lex_exponent() {
                return Type::MalformedNumber;
            }
            is_float = true;
        }
        self.consume_cdata_suffix();
        self.finish_number(if is_float { Type::Float } else { Type::Integer })
    }

    /// Consumes `[eEpP] [+-]? digits`; the exponent is always decimal.
    fn lex_exponent(&mut self) -> bool {
        self.advance();
        if matches!(self.current, b'+' | b'-') {
            self.advance();
        }
        if !self.current.is_ascii_digit() {
            return false;
        }
        self.skip_digits(u8::is_ascii_digit);
        true
    }

    /// Consumes a LuaJIT `LL`/`ULL`/`LLU`/`i` suffix, case-insensitively.
    #[inline]
    fn consume_cdata_suffix(&mut self) {
        #[cfg(feature = "luajit")]
        if self.dialect.has_cdata_suffixes() {
            match self.current.to_ascii_lowercase() {
                b'i' => {
                    self.advance();
                }
                b'u' => {
                    self.advance();
                    for _ in 0..2 {
                        if matches!(self.current, b'l' | b'L') {
                            self.advance();
                        }
                    }
                }
                b'l' => {
                    self.advance();
                    if matches!(self.current, b'l' | b'L') {
                        self.advance();
                    }
                    if matches!(self.current, b'u' | b'U') {
                        self.advance();
                    }
                }
                _ => {}
            }
        }
    }

    fn lex_word(&mut self, start: u32) -> Type {
        let extended = self.dialect.has_extended_identifiers();
        while is_word_byte(self.current) || (extended && self.current >= 0x80) {
            self.advance();
        }
        keyword(&self.source[start as usize..self.position as usize]).unwrap_or(Type::Identifier)
    }

    #[inline(always)]
    fn create_token(&self, token_type: Type, start: u32) -> Token {
        Token {
            token_type,
            span: Span::new(start, self.position),
        }
    }
}

#[inline(always)]
fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

#[cfg(feature = "compound")]
#[inline(always)]
fn compound_token(base: Type) -> Option<Type> {
    Some(match base {
        Type::Plus => Type::PlusAssign,
        Type::Minus => Type::MinusAssign,
        Type::Star => Type::StarAssign,
        Type::Slash => Type::SlashAssign,
        Type::DoubleSlash => Type::DoubleSlashAssign,
        Type::Percent => Type::PercentAssign,
        Type::Caret => Type::CaretAssign,
        Type::DoubleDot => Type::DoubleDotAssign,
        _ => return None,
    })
}

/// Lua's reserved words, shared by every dialect. Luau's contextual keywords
/// (`type`, `export`, `continue`, ...) lex as identifiers.
#[inline(always)]
fn keyword(word: &[u8]) -> Option<Type> {
    Some(match word {
        b"and" => Type::And,
        b"break" => Type::Break,
        b"do" => Type::Do,
        b"else" => Type::Else,
        b"elseif" => Type::Elseif,
        b"end" => Type::End,
        b"false" => Type::False,
        b"for" => Type::For,
        b"function" => Type::Function,
        b"goto" => Type::Goto,
        b"if" => Type::If,
        b"in" => Type::In,
        b"local" => Type::Local,
        b"nil" => Type::Nil,
        b"not" => Type::Not,
        b"or" => Type::Or,
        b"repeat" => Type::Repeat,
        b"return" => Type::Return,
        b"then" => Type::Then,
        b"true" => Type::True,
        b"until" => Type::Until,
        b"while" => Type::While,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(source: &str, dialect: Dialect) -> std::vec::Vec<(Type, &str)> {
        let arena = Bump::new();
        Lexer::new(source, &arena, dialect)
            .lex()
            .iter()
            .map(|token| (token.token_type, &source[token.span.range()]))
            .collect()
    }

    #[test]
    fn all_is_indexed_by_discriminant() {
        for (index, kind) in Type::ALL.iter().enumerate() {
            assert_eq!(*kind as usize, index, "{}", kind.name());
        }
    }

    #[test]
    fn form_feed_and_vertical_tab_are_whitespace() {
        assert_eq!(
            lex("a\x0bb\x0cc", Dialect::Lua),
            [
                (Type::Identifier, "a"),
                (Type::Identifier, "b"),
                (Type::Identifier, "c")
            ]
        );
    }

    #[test]
    fn carriage_return_ends_a_short_string() {
        assert_eq!(lex("'a\rb'", Dialect::Lua)[0].0, Type::UnterminatedString);
    }

    #[test]
    fn unterminated_string_stops_before_the_line_ending() {
        assert_eq!(
            lex("'ab\nc", Dialect::Lua),
            [(Type::UnterminatedString, "'ab"), (Type::Identifier, "c")]
        );
    }

    #[test]
    fn nul_byte_inside_a_string_is_content() {
        assert_eq!(lex("'a\0b'", Dialect::Lua), [(Type::String, "'a\0b'")]);
    }

    #[test]
    fn unexpected_multibyte_character_is_one_token() {
        assert_eq!(lex("é", Dialect::Lua), [(Type::UnexpectedByte, "é")]);
    }

    #[cfg(feature = "luajit")]
    #[test]
    fn luajit_accepts_non_ascii_identifiers() {
        assert_eq!(lex("été", Dialect::LuaJit), [(Type::Identifier, "été")]);
    }

    #[test]
    fn a_radix_prefix_needs_digits() {
        for source in ["0x", "0x.", "0x.p1", "0b"] {
            assert_eq!(lex(source, Dialect::Lua), [(Type::MalformedNumber, source)]);
        }
        #[cfg(feature = "luau")]
        assert_eq!(lex("0x_", Dialect::Luau), [(Type::MalformedNumber, "0x_")]);
        assert_eq!(lex("0x.8", Dialect::Lua), [(Type::Float, "0x.8")]);
    }

    #[test]
    fn digit_separators_are_luau_only() {
        assert_eq!(
            lex("1_000", Dialect::Lua),
            [(Type::MalformedNumber, "1_000")]
        );
        #[cfg(feature = "luau")]
        assert_eq!(lex("1_000", Dialect::Luau), [(Type::Integer, "1_000")]);
    }

    #[test]
    fn span_display() {
        assert_eq!(Span::new(3, 7).to_string(), "3..7");
    }
}
