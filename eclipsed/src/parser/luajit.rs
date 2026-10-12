//! LuaJIT cdata literal suffixes (`LL`, `ULL`, `i`).

use crate::ast::Expression;
use crate::lexer::Token;
use crate::parser::Parser;
use crate::parser::number::{parse_float, parse_uint};

impl<'source, 'arena, 'interner> Parser<'source, 'arena, 'interner> {
    /// The cdata literal `token` denotes, or `None` if it has no suffix.
    pub(crate) fn parse_cdata_literal(&self, token: &Token) -> Option<Expression<'arena>> {
        let text = self.token_str(token);
        let span = token.span;

        if ends_with_ci(text, "ull") || ends_with_ci(text, "llu") {
            let value = parse_uint(&text[..text.len() - 3])?;
            return Some(Expression::U64 { value, span });
        }
        if ends_with_ci(text, "ll") {
            let value = parse_uint(&text[..text.len() - 2])?;
            return Some(Expression::I64 {
                value: value as i64,
                span,
            });
        }
        if ends_with_ci(text, "i") {
            let value = parse_float(&text[..text.len() - 1])?;
            return Some(Expression::Imaginary { value, span });
        }
        None
    }
}

fn ends_with_ci(text: &str, suffix: &str) -> bool {
    text.len() >= suffix.len()
        && text.as_bytes()[text.len() - suffix.len()..].eq_ignore_ascii_case(suffix.as_bytes())
}
