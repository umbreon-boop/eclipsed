//! Vanilla Lua 5.4 (`Dialect::Lua`) — always compiled, needs no features.

mod common;

use common::{parse_with, return_value};
use eclipsed::ast::*;
use eclipsed::dialect::Dialect;

fn parse(
    source: &str,
    assert_fn: impl FnOnce(&[Statement<'_>], &eclipsed::interner::Interner<'_>),
) {
    parse_with(source, Dialect::Lua, assert_fn)
}

#[test]
fn arithmetic_precedence_and_associativity() {
    // `+` < `*` < `^`, and `^` is right-associative:
    // 1 + 2 * 3 ^ 2  ==  1 + (2 * (3 ^ 2))
    parse("return 1 + 2 * 3 ^ 2", |statements, _| {
        let Expression::Binary {
            left,
            operator: BinaryOperator::Add,
            right,
            ..
        } = return_value(statements)
        else {
            panic!("expected top-level Add");
        };
        assert!(matches!(**left, Expression::Integer { value: 1, .. }));

        let Expression::Binary {
            left: mul_left,
            operator: BinaryOperator::Multiply,
            right: mul_right,
            ..
        } = &**right
        else {
            panic!("expected Multiply on the right of Add");
        };
        assert!(matches!(**mul_left, Expression::Integer { value: 2, .. }));
        assert!(matches!(
            **mul_right,
            Expression::Binary {
                operator: BinaryOperator::Power,
                ..
            }
        ));
    });
}

#[test]
fn power_is_right_associative() {
    parse("return 2 ^ 3 ^ 2", |statements, _| {
        let Expression::Binary {
            left,
            operator: BinaryOperator::Power,
            right,
            ..
        } = return_value(statements)
        else {
            panic!("expected Power at the root");
        };
        assert!(matches!(**left, Expression::Integer { value: 2, .. }));
        assert!(matches!(
            **right,
            Expression::Binary {
                operator: BinaryOperator::Power,
                ..
            }
        ));
    });
}

#[test]
fn concatenation_is_right_associative() {
    parse("return a .. b .. c", |statements, _| {
        let Expression::Binary {
            operator: BinaryOperator::Concatenate,
            right,
            ..
        } = return_value(statements)
        else {
            panic!("expected Concatenate at the root");
        };
        assert!(matches!(
            **right,
            Expression::Binary {
                operator: BinaryOperator::Concatenate,
                ..
            }
        ));
    });
}

#[test]
fn if_elseif_else() {
    let source = "if a then return 1 elseif b then return 2 else return 3 end";
    parse(source, |statements, _| {
        let Statement::If {
            then_block,
            elseifs,
            else_block,
            ..
        } = &statements[0]
        else {
            panic!("expected an if statement");
        };
        assert_eq!(then_block.len(), 1);
        assert_eq!(elseifs.len(), 1);
        assert!(else_block.is_some());
    });
}

#[test]
fn numeric_for() {
    parse("for i = 1, 10, 2 do end", |statements, interner| {
        let Statement::NumericFor { variable, step, .. } = &statements[0] else {
            panic!("expected a numeric for");
        };
        assert_eq!(interner.resolve(*variable), "i");
        assert!(step.is_some(), "explicit step provided");
    });
}

#[test]
fn generic_for() {
    parse("for k, v in pairs(t) do end", |statements, interner| {
        let Statement::GenericFor {
            variables,
            iterators,
            ..
        } = &statements[0]
        else {
            panic!("expected a generic for");
        };
        assert_eq!(variables.len(), 2);
        assert_eq!(interner.resolve(variables[0].symbol), "k");
        assert_eq!(interner.resolve(variables[1].symbol), "v");
        assert_eq!(iterators.len(), 1);
    });
}

#[test]
fn method_call() {
    parse("obj:method(1, 2)", |statements, interner| {
        let Statement::Expression { expr, .. } = &statements[0] else {
            panic!("expected an expression statement");
        };
        let Expression::MethodCall {
            method, arguments, ..
        } = expr
        else {
            panic!("expected a method call");
        };
        assert_eq!(interner.resolve(*method), "method");
        let CallArguments::Expressions { arguments, .. } = arguments else {
            panic!("expected positional arguments");
        };
        assert_eq!(arguments.len(), 2);
    });
}

#[test]
fn table_constructor_field_kinds() {
    parse("local t = { 1, x = 3, [k] = 4 }", |statements, interner| {
        let Statement::Local { values, .. } = &statements[0] else {
            panic!("expected a local statement");
        };
        let Expression::Table { fields, .. } = &values[0] else {
            panic!("expected a table constructor");
        };
        assert_eq!(fields.len(), 3);
        assert!(matches!(fields[0].key, TableKey::None));
        match &fields[1].key {
            TableKey::Name { symbol, .. } => assert_eq!(interner.resolve(*symbol), "x"),
            other => panic!("expected a named key, got {other:?}"),
        }
        assert!(matches!(fields[2].key, TableKey::Expression(_)));
    });
}

#[test]
fn while_loop_with_break() {
    parse("while a do break end", |statements, _| {
        let Statement::While { body, .. } = &statements[0] else {
            panic!("expected a while loop");
        };
        assert_eq!(body.len(), 1);
        assert!(matches!(body[0], Statement::Break { .. }));
    });
}

#[test]
fn repeat_until() {
    parse("repeat x = 1 until done", |statements, _| {
        let Statement::Repeat { body, until, .. } = &statements[0] else {
            panic!("expected a repeat statement");
        };
        assert_eq!(body.len(), 1);
        assert!(matches!(until, Expression::Identifier { .. }));
    });
}

#[test]
fn do_block() {
    parse("do local x = 1 end", |statements, _| {
        let Statement::Do { body, .. } = &statements[0] else {
            panic!("expected a do block");
        };
        assert_eq!(body.len(), 1);
        assert!(matches!(body[0], Statement::Local { .. }));
    });
}

#[test]
fn goto_and_label() {
    parse("::top:: goto top", |statements, interner| {
        assert_eq!(statements.len(), 2);
        let Statement::Label { symbol, .. } = &statements[0] else {
            panic!("expected a label");
        };
        assert_eq!(interner.resolve(*symbol), "top");
        let Statement::Goto { label, .. } = &statements[1] else {
            panic!("expected a goto");
        };
        assert_eq!(interner.resolve(*label), "top");
    });
}

#[test]
fn multiple_assignment() {
    parse("a, b = 1, 2", |statements, _| {
        let Statement::Assign {
            targets, values, ..
        } = &statements[0]
        else {
            panic!("expected an assignment");
        };
        assert_eq!(targets.len(), 2);
        assert_eq!(values.len(), 2);
        assert!(matches!(values[0], Expression::Integer { value: 1, .. }));
        assert!(matches!(values[1], Expression::Integer { value: 2, .. }));
    });
}

#[test]
fn closure_expression() {
    parse(
        "local f = function(x) return x end",
        |statements, interner| {
            let Statement::Local { values, .. } = &statements[0] else {
                panic!("expected a local statement");
            };
            let Expression::Closure { body, .. } = &values[0] else {
                panic!("expected a closure expression");
            };
            assert_eq!(body.parameters.len(), 1);
            assert_eq!(interner.resolve(body.parameters[0].symbol), "x");
        },
    );
}

#[test]
fn spans_cover_source_text() {
    let source = "return 12345";
    parse(source, |statements, _| {
        let Expression::Integer { span, .. } = return_value(statements) else {
            panic!("expected an integer literal");
        };
        assert_eq!(&source[span.start as usize..span.end as usize], "12345");

        let stmt_span = statements[0].span();
        assert_eq!(
            &source[stmt_span.start as usize..stmt_span.end as usize],
            "return 12345"
        );
    });
}

#[test]
fn trailing_whitespace_and_newline_are_ignored() {
    // Regression: trailing trivia must not produce a spurious token at EOF.
    parse("return 1\n", |statements, _| {
        assert_eq!(statements.len(), 1);
        assert!(matches!(statements[0], Statement::Return { .. }));
    });
    parse("local x = 1   \n\t\n", |statements, _| {
        assert_eq!(statements.len(), 1);
    });
}

#[test]
fn trivia_only_source_parses_empty() {
    // A file of only whitespace and comments has no statements.
    parse("  -- a comment\n--[[ block ]]\n  ", |statements, _| {
        assert!(statements.is_empty());
    });
}

#[test]
fn parentheses_are_preserved() {
    // `(f())` must keep its Paren node (it truncates to one value).
    parse("return (f())", |statements, _| {
        let Expression::Paren { expression, .. } = return_value(statements) else {
            panic!("expected a parenthesized expression");
        };
        assert!(matches!(**expression, Expression::Call { .. }));
    });
}

#[test]
fn string_literals_decode_via_public_api() {
    use eclipsed::string::decode_literal;
    assert_eq!(decode_literal(r#""\072\105""#), b"Hi");
    assert_eq!(decode_literal(r#""a\tb""#), b"a\tb");
    assert_eq!(decode_literal("[[raw\\n]]"), b"raw\\n"); // long strings are verbatim
    assert_eq!(decode_literal(r#""\x41\x42""#), b"AB");
}

#[test]
fn type_keywords_are_identifiers_in_vanilla() {
    // `record`, `type`, `as` etc. are ordinary identifiers in vanilla Lua.
    parse("local record = 1", |statements, interner| {
        let Statement::Local { variables, .. } = &statements[0] else {
            panic!("expected a local statement");
        };
        assert_eq!(interner.resolve(variables[0].symbol), "record");
    });
}

// ---- `prefixexp` suffix discipline ----
//
// Lua hangs the `.name` / `[key]` / `:m(...)` / call suffixes off `prefixexp`
// (a name or a parenthesised expression) only. Attaching them to any operand
// makes the parser swallow the statement that follows a value.

#[test]
fn suffixes_only_attach_to_prefix_expressions() {
    for source in [
        "return 34(s)",         // a numeral is not callable syntax
        "return 34 {}",         // ...nor with table-constructor arguments
        r#"return "a" "b""#,    // adjacent strings are not a call
        r#"return "a":rep(2)"#, // needs ("a"):rep(2)
        "return {}[1]",
        "return {}.x",
        "return nil.x",
        "return true:foo()",
        "return ...(1)",
        "return function() end()", // needs (function() end)()
    ] {
        common::parse_error(source, Dialect::Lua);
    }
}

#[test]
fn suffixes_chain_off_a_parenthesized_base() {
    // `(...)` *is* a prefixexp, so suffixes are legal on it.
    parse("return (f())[1].x:m()", |statements, _| {
        let Expression::MethodCall { receiver, .. } = return_value(statements) else {
            panic!("expected a method call at the root");
        };
        let Expression::Field { table, .. } = &**receiver else {
            panic!("expected a field access under the method call");
        };
        let Expression::Index { table, .. } = &**table else {
            panic!("expected an index under the field access");
        };
        assert!(matches!(**table, Expression::Paren { .. }));
    });
}

#[test]
fn a_value_ends_the_statement_before_a_parenthesized_target() {
    // `34` cannot take call arguments, so the `(` opens a fresh statement.
    // Getting this wrong parses all three lines as one assignment and then
    // fails on the second `=`.
    let source = "local L\nL = 34\n(s)[50], s[2] = 1, 2";
    parse(source, |statements, _| {
        assert_eq!(statements.len(), 3, "got {statements:#?}");

        let Statement::Assign {
            targets, values, ..
        } = &statements[1]
        else {
            panic!("expected `L = 34`, got {:?}", statements[1]);
        };
        assert_eq!(targets.len(), 1);
        assert!(matches!(values[0], Expression::Integer { value: 34, .. }));

        let Statement::Assign {
            targets, values, ..
        } = &statements[2]
        else {
            panic!("expected the `(s)[50], s[2] = ...` assignment");
        };
        assert_eq!(targets.len(), 2);
        assert_eq!(values.len(), 2);
        let Expression::Index { table, .. } = &targets[0] else {
            panic!("expected `(s)[50]` to be an index");
        };
        assert!(matches!(**table, Expression::Paren { .. }));
    });
}

#[test]
fn vanilla_lua_reads_a_call_across_a_newline() {
    // Lua 5.1 called this "ambiguous syntax"; 5.2+ (and full-moon) resolve it
    // greedily as a call. Only `Dialect::Luau` diverges — see the luau tests.
    parse("f\n(x)", |statements, _| {
        assert_eq!(statements.len(), 1, "got {statements:#?}");
        let Statement::Expression { expr, .. } = &statements[0] else {
            panic!("expected a call statement, got {:?}", statements[0]);
        };
        assert!(matches!(**expr, Expression::Call { .. }));
    });
}

// ---- assignment targets ----

#[test]
fn assignment_targets_must_be_variables() {
    for source in [
        "f() = 1", // a call is not assignable
        "a:m() = 1",
        "1 = 2",
        "(a) = 1",       // a prefixexp, but not a `var`
        "a, f() = 1, 2", // the check covers every target, not just the first
        "-a = 1",
    ] {
        common::parse_error(source, Dialect::Lua);
    }
}

#[test]
fn assignable_targets_are_names_fields_and_indexes() {
    parse("a, b.c, d[1] = 1, 2, 3", |statements, _| {
        let Statement::Assign { targets, .. } = &statements[0] else {
            panic!("expected an assignment, got {:?}", statements[0]);
        };
        assert!(matches!(targets[0], Expression::Identifier { .. }));
        assert!(matches!(targets[1], Expression::Field { .. }));
        assert!(matches!(targets[2], Expression::Index { .. }));
    });
}

// ---- numeric-literal overflow (Lua 5.4 §3.1) ----

#[test]
fn an_overflowing_decimal_integer_denotes_a_float() {
    // "a decimal integer numeral that overflows ... denotes a float". Rejecting
    // it would refuse input real Lua accepts; parsing it as `u64` would store a
    // bit pattern consumers reinterpret as negative.
    parse("return 9223372036854775808", |statements, _| {
        let Expression::Float { value, .. } = return_value(statements) else {
            panic!("expected a float, got {:?}", return_value(statements));
        };
        assert_eq!(*value, 9223372036854775808.0);
    });
    parse("return 18446744073709551615", |statements, _| {
        assert!(matches!(return_value(statements), Expression::Float { .. }));
    });
}

#[test]
fn the_largest_representable_integer_stays_an_integer() {
    parse("return 9223372036854775807", |statements, _| {
        let Expression::Integer { value, .. } = return_value(statements) else {
            panic!("expected an integer, got {:?}", return_value(statements));
        };
        assert_eq!(*value, i64::MAX);
    });
}

#[test]
fn an_overflowing_hex_integer_wraps_instead() {
    // Hex is specified differently from decimal: "if the value overflows, it
    // wraps around to fit into a valid integer".
    parse("return 0xFFFFFFFFFFFFFFFF", |statements, _| {
        let Expression::Integer { value, .. } = return_value(statements) else {
            panic!("expected an integer, got {:?}", return_value(statements));
        };
        assert_eq!(*value, -1);
    });
    parse("return 0xFFFFFFFFFFFFFFFFF", |statements, _| {
        let Expression::Integer { value, .. } = return_value(statements) else {
            panic!("expected an integer");
        };
        assert_eq!(*value, -1, "17 hex digits wrap to the same value");
    });
}

#[test]
fn a_huge_literal_from_an_unparsed_float_round_trips() {
    // The shape that broke corona on samples/luarmor.lua: a float printed as 85
    // decimal digits re-lexes as an integer far beyond `u64`.
    let digits = format!("return {}", "5".to_string() + &"0".repeat(84));
    parse(&digits, |statements, _| {
        assert!(matches!(return_value(statements), Expression::Float { .. }));
    });
}

#[test]
fn error_at_end_of_input_points_at_the_end() {
    let source = "local x = ";
    let arena = bumpalo::Bump::new();
    let tokens = eclipsed::lexer::Lexer::new(source, &arena, Dialect::Lua).lex();
    let mut interner = eclipsed::interner::Interner::new();
    let error = eclipsed::parser::Parser::new(tokens, source, &arena, &mut interner, Dialect::Lua)
        .parse()
        .unwrap_err();
    let end = source.len() as u32;
    assert_eq!(error.span(), eclipsed::lexer::Span::new(end, end));
}

#[test]
fn invalid_attribute_names_the_attribute() {
    let error = common::parse_error("local x <frozen> = 1", Dialect::Lua);
    assert!(error.contains("'frozen'"), "{error}");
}

#[test]
fn deep_nesting_is_an_error_not_a_stack_overflow() {
    let depth = 100_000;
    let expression = format!("return {}1{}", "(".repeat(depth), ")".repeat(depth));
    assert!(common::parse_error(&expression, Dialect::Lua).contains("nesting"));

    let blocks = format!("{}{}", "do ".repeat(depth), "end ".repeat(depth));
    assert!(common::parse_error(&blocks, Dialect::Lua).contains("nesting"));

    let unary = format!("return {}1", "not ".repeat(depth));
    assert!(common::parse_error(&unary, Dialect::Lua).contains("nesting"));
}

#[test]
fn nesting_within_the_limit_parses_on_a_small_stack() {
    let depth = eclipsed::parser::MAX_DEPTH as usize / 6;
    let source = format!(
        "{}return {}1{}{}",
        "f(function() ".repeat(depth),
        "(".repeat(depth),
        ")".repeat(depth),
        "end) ".repeat(depth),
    );
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || parse(&source, |statements, _| assert_eq!(statements.len(), 1)))
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn a_function_expression_span_covers_the_keyword() {
    let source = "return function(a) end";
    parse(source, |statements, _| {
        let span = return_value(statements).span();
        assert_eq!(
            &source[span.start as usize..span.end as usize],
            "function(a) end"
        );
    });
}

#[test]
fn goto_is_a_valid_function_field_and_method_name() {
    for source in [
        "function a.goto() end",
        "function a:goto() end",
        "a.goto()",
        "a:goto()",
    ] {
        parse(source, |_, _| {});
    }
}
