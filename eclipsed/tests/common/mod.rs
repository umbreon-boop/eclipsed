//! Shared test harness for the dialect-specific integration test crates.
//!
//! Each `tests/*.rs` crate pulls this in with `mod common;`. Items unused by a
//! given crate are expected, hence the blanket `dead_code` allow.
#![allow(dead_code)]

use bumpalo::Bump;
use eclipsed::ast::*;
use eclipsed::dialect::Dialect;
use eclipsed::interner::Interner;
use eclipsed::lexer::Lexer;
use eclipsed::parser::Parser;

/// Lex + parse `source` in `dialect`, then hand the AST and a resolvable
/// interner to `assert_fn`. The closure form keeps the arena, interner and AST
/// alive together while avoiding lifetime gymnastics at each call site.
pub fn parse_with<R>(
    source: &str,
    dialect: Dialect,
    assert_fn: impl FnOnce(&[Statement<'_>], &Interner<'_>) -> R,
) -> R {
    let arena = Bump::new();
    let tokens = Lexer::new(source, &arena, dialect).lex();
    let mut interner = Interner::new();
    let statements = {
        let mut parser = Parser::new(tokens, source, &arena, &mut interner, dialect);
        parser.parse().expect("parse should succeed")
    };
    assert_fn(statements, &interner)
}

/// Lex + parse `source` in `dialect` and return the error it is rejected with.
/// Panics when the source parses, so a caller can assert rejection without
/// pinning down the exact message.
pub fn parse_error(source: &str, dialect: Dialect) -> String {
    let arena = Bump::new();
    let tokens = Lexer::new(source, &arena, dialect).lex();
    let mut interner = Interner::new();
    let mut parser = Parser::new(tokens, source, &arena, &mut interner, dialect);
    match parser.parse() {
        Ok(statements) => panic!("expected {source:?} to be rejected, parsed {statements:#?}"),
        Err(error) => error.to_string(),
    }
}

/// The single value of a `return <expr>` statement — a convenient, always-valid
/// way to probe expression parsing.
pub fn return_value<'a, 'arena>(statements: &'a [Statement<'arena>]) -> &'a Expression<'arena> {
    let Statement::Return { values, .. } = &statements[0] else {
        panic!("expected a return statement, got {:?}", statements[0]);
    };
    assert_eq!(values.len(), 1, "expected exactly one returned value");
    &values[0]
}
