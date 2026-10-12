//! LuaJIT dialect (`Dialect::LuaJit`) — 64-bit/cdata literal suffixes,
//! imaginary literals, and hex floats. Compiled only with `--features luajit`.
#![cfg(feature = "luajit")]

mod common;

use common::{parse_with, return_value};
use eclipsed::ast::*;
use eclipsed::dialect::Dialect;

fn parse(
    source: &str,
    assert_fn: impl FnOnce(&[Statement<'_>], &eclipsed::interner::Interner<'_>),
) {
    parse_with(source, Dialect::LuaJit, assert_fn)
}

#[test]
fn signed_64bit_literal() {
    parse("return 42LL", |statements, _| {
        let Expression::I64 { value, .. } = return_value(statements) else {
            panic!(
                "expected an I64 literal, got {:?}",
                return_value(statements)
            );
        };
        assert_eq!(*value, 42);
    });
}

#[test]
fn unsigned_64bit_hex_literal() {
    parse("return 0xffULL", |statements, _| {
        let Expression::U64 { value, .. } = return_value(statements) else {
            panic!("expected a U64 literal");
        };
        assert_eq!(*value, 255);
    });
}

#[test]
fn imaginary_float_literal() {
    parse("return 12.5i", |statements, _| {
        let Expression::Imaginary { value, .. } = return_value(statements) else {
            panic!("expected an imaginary literal");
        };
        assert_eq!(*value, 12.5);
    });
}

#[test]
fn imaginary_integer_literal() {
    parse("return 3i", |statements, _| {
        let Expression::Imaginary { value, .. } = return_value(statements) else {
            panic!("expected an imaginary literal");
        };
        assert_eq!(*value, 3.0);
    });
}

#[test]
fn hex_float_literal() {
    // 0x1.8p1 == (1 + 8/16) * 2^1 == 3.0
    parse("return 0x1.8p1", |statements, _| {
        let Expression::Float { value, .. } = return_value(statements) else {
            panic!(
                "expected a float literal, got {:?}",
                return_value(statements)
            );
        };
        assert_eq!(*value, 3.0);
    });
}

#[test]
fn lowercase_suffix() {
    parse("return 7ll", |statements, _| {
        let Expression::I64 { value, .. } = return_value(statements) else {
            panic!("expected an I64 literal");
        };
        assert_eq!(*value, 7);
    });
}

#[test]
fn plain_numbers_still_parse() {
    parse("return 0xff", |statements, _| {
        let Expression::Integer { value, .. } = return_value(statements) else {
            panic!("expected a plain integer");
        };
        assert_eq!(*value, 255);
    });
}

#[test]
fn span_excludes_nothing_for_suffix() {
    // The token (and AST span) covers the whole `42LL`.
    let source = "return 42LL";
    parse(source, |statements, _| {
        let expr = return_value(statements);
        let span = expr.span();
        assert_eq!(&source[span.start as usize..span.end as usize], "42LL");
    });
}

#[test]
fn realistic_multi_statement_program() {
    let source = "\
local big = 42LL
local mask = 0xffULL
local z = 1.5i
local f = 0x1.8p1
goto done
::done::
local sum = big";
    parse(source, |statements, _| {
        assert_eq!(statements.len(), 7, "seven top-level statements");

        let value_of = |index: usize| -> &Expression<'_> {
            let Statement::Local { values, .. } = &statements[index] else {
                panic!("expected a local at {index}");
            };
            &values[0]
        };
        assert!(matches!(value_of(0), Expression::I64 { value: 42, .. }));
        assert!(matches!(value_of(1), Expression::U64 { value: 255, .. }));
        assert!(matches!(value_of(2), Expression::Imaginary { .. }));
        assert!(matches!(value_of(3), Expression::Float { .. }));
        assert!(matches!(statements[4], Statement::Goto { .. }));
        assert!(matches!(statements[5], Statement::Label { .. }));
        assert!(matches!(statements[6], Statement::Local { .. }));
    });
}

#[test]
fn goto_and_label_supported() {
    // LuaJIT backports goto/labels from 5.2.
    parse("::top:: goto top", |statements, interner| {
        assert_eq!(statements.len(), 2);
        let Statement::Label { symbol, .. } = &statements[0] else {
            panic!("expected a label");
        };
        assert_eq!(interner.resolve(*symbol), "top");
    });
}
