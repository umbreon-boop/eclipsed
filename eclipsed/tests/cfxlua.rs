//! CfxLua (FiveM) — needs `--features cfxlua`.
#![cfg(feature = "cfxlua")]

mod common;

use common::{parse_with, return_value};
use eclipsed::ast::*;
use eclipsed::dialect::Dialect;
use eclipsed::interner::Interner;

fn parse(source: &str, assert_fn: impl FnOnce(&[Statement<'_>], &Interner<'_>)) {
    parse_with(source, Dialect::CfxLua, assert_fn)
}

#[test]
fn bitwise_compound_assignment() {
    parse("x <<= 2", |statements, _| {
        let Statement::CompoundAssign {
            operator: CompoundOperator::ShiftLeft,
            ..
        } = &statements[0]
        else {
            panic!("expected <<= compound assignment, got {:?}", statements[0]);
        };
    });
}

#[test]
fn safe_navigation_field() {
    parse("return a?.b?.c", |statements, interner| {
        let Expression::Field {
            field, safe: true, ..
        } = return_value(statements)
        else {
            panic!("expected safe field access");
        };
        assert_eq!(interner.resolve(*field), "c");
    });
}

#[test]
fn plain_field_is_not_safe() {
    parse("return a.b", |statements, _| {
        assert!(matches!(
            return_value(statements),
            Expression::Field { safe: false, .. }
        ));
    });
}

#[test]
fn set_constructor_desugars_to_true() {
    parse("return { .a, b = false }", |statements, interner| {
        let Expression::Table { fields, .. } = return_value(statements) else {
            panic!("expected a table");
        };
        assert_eq!(fields.len(), 2);
        let TableKey::Name { symbol, .. } = fields[0].key else {
            panic!("expected a name key");
        };
        assert_eq!(interner.resolve(symbol), "a");
        assert!(matches!(fields[0].value, Expression::True { .. }));
    });
}

#[test]
fn in_unpacking() {
    parse("local a, b in t", |statements, interner| {
        let Statement::InUnpack {
            variables, source, ..
        } = &statements[0]
        else {
            panic!("expected in-unpacking, got {:?}", statements[0]);
        };
        assert_eq!(variables.len(), 2);
        assert_eq!(interner.resolve(variables[1].symbol), "b");
        assert!(matches!(source, Expression::Identifier { .. }));
    });
}

#[test]
fn jenkins_hash_string() {
    parse("return `WEAPON_PISTOL`", |statements, interner| {
        let Expression::JenkinsHash { value, .. } = return_value(statements) else {
            panic!("expected a jenkins hash literal");
        };
        assert_eq!(interner.resolve(*value), "`WEAPON_PISTOL`");
    });
}

#[test]
fn c_style_comments_are_trivia() {
    parse(
        "return 1 /* trailing */ + /* inline */ 2",
        |statements, _| {
            assert!(matches!(
                return_value(statements),
                Expression::Binary {
                    operator: BinaryOperator::Add,
                    ..
                }
            ));
        },
    );
}

#[test]
fn floor_division_still_works() {
    // `//` must stay floor division, not become a C-style line comment.
    parse("return 7 // 2", |statements, _| {
        assert!(matches!(
            return_value(statements),
            Expression::Binary {
                operator: BinaryOperator::FloorDivide,
                ..
            }
        ));
    });
}
