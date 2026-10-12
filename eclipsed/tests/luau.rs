//! Luau dialect (`Dialect::Luau`) — compound assignment, `continue`, `if`
//! expressions, backtick interpolation, type aliases and type extensions.
//! Compiled only with `--features luau`.
#![cfg(feature = "luau")]

mod common;

use common::{parse_with, return_value};
use eclipsed::ast::*;
use eclipsed::dialect::Dialect;

fn parse(
    source: &str,
    assert_fn: impl FnOnce(&[Statement<'_>], &eclipsed::interner::Interner<'_>),
) {
    parse_with(source, Dialect::Luau, assert_fn)
}

#[test]
fn compound_assignment_add() {
    parse("a += 1", |statements, interner| {
        let Statement::CompoundAssign {
            target,
            operator,
            value,
            ..
        } = &statements[0]
        else {
            panic!("expected a compound assignment, got {:?}", statements[0]);
        };
        assert_eq!(*operator, CompoundOperator::Add);
        let Expression::Identifier { name, .. } = target else {
            panic!("expected identifier target");
        };
        assert_eq!(interner.resolve(*name), "a");
        assert!(matches!(value, Expression::Integer { value: 1, .. }));
    });
}

#[test]
fn compound_assignment_concat() {
    parse("s ..= \"x\"", |statements, _| {
        let Statement::CompoundAssign { operator, .. } = &statements[0] else {
            panic!("expected a compound assignment");
        };
        assert_eq!(*operator, CompoundOperator::Concatenate);
    });
}

#[test]
fn continue_statement_in_loop() {
    parse("for i = 1, 3 do continue end", |statements, _| {
        let Statement::NumericFor { body, .. } = &statements[0] else {
            panic!("expected a numeric for");
        };
        assert_eq!(body.len(), 1);
        assert!(matches!(body[0], Statement::Continue { .. }));
    });
}

#[test]
fn continue_allows_trailing_semicolon() {
    // `continue` is a block terminator, but a trailing empty statement before
    // the enclosing `else`/`end` is legal (Luraph emits `then continue; else`).
    parse(
        "while true do if x then continue; else return end end",
        |statements, _| {
            assert!(matches!(statements[0], Statement::While { .. }));
        },
    );
}

#[test]
fn continue_is_contextual_identifier() {
    // `continue` followed by `=` is an ordinary variable, not the keyword.
    parse("continue = 5", |statements, interner| {
        let Statement::Assign { targets, .. } = &statements[0] else {
            panic!("expected an assignment, got {:?}", statements[0]);
        };
        let Expression::Identifier { name, .. } = &targets[0] else {
            panic!("expected identifier target");
        };
        assert_eq!(interner.resolve(*name), "continue");
    });
}

#[test]
fn if_expression() {
    parse("local x = if c then 1 else 2", |statements, _| {
        let Statement::Local { values, .. } = &statements[0] else {
            panic!("expected a local");
        };
        let Expression::IfElse {
            then_branch,
            else_branch,
            elseifs,
            ..
        } = &values[0]
        else {
            panic!("expected an if expression, got {:?}", values[0]);
        };
        assert!(elseifs.is_empty());
        assert!(matches!(
            **then_branch,
            Expression::Integer { value: 1, .. }
        ));
        assert!(matches!(
            **else_branch,
            Expression::Integer { value: 2, .. }
        ));
    });
}

#[test]
fn if_expression_with_elseif() {
    parse(
        "return if a then 1 elseif b then 2 else 3",
        |statements, _| {
            let Expression::IfElse { elseifs, .. } = return_value(statements) else {
                panic!("expected an if expression");
            };
            assert_eq!(elseifs.len(), 1);
        },
    );
}

#[test]
fn if_conditions_can_bind_local_and_const_names() {
    parse(
        "if local player: Player? = getPlayer() then print(player) elseif const guest = findGuest() then print(guest) end",
        |statements, interner| {
            let Statement::If {
                binding, elseifs, ..
            } = &statements[0]
            else {
                panic!("expected an if statement");
            };
            let binding = binding.as_ref().expect("local binding");
            assert!(!binding.is_const);
            assert_eq!(interner.resolve(binding.variable.symbol), "player");
            assert!(binding.variable.type_annotation.is_some());

            let binding = elseifs[0].binding.as_ref().expect("const binding");
            assert!(binding.is_const);
            assert_eq!(interner.resolve(binding.variable.symbol), "guest");
        },
    );
}

#[test]
fn if_expression_can_bind_in_each_arm() {
    parse(
        "local result = if local player = getPlayer() then player.Name elseif const guest = getGuest() then guest.Name else \"unknown\"",
        |statements, interner| {
            let Statement::Local { values, .. } = &statements[0] else {
                panic!("expected a local");
            };
            let Expression::IfElse {
                binding, elseifs, ..
            } = &values[0]
            else {
                panic!("expected an if expression");
            };
            assert_eq!(
                interner.resolve(binding.as_ref().unwrap().variable.symbol),
                "player"
            );
            assert!(elseifs[0].binding.as_ref().unwrap().is_const);
        },
    );
}

#[test]
fn string_interpolation() {
    // `a{x}b` => [Literal("a"), Expr(x), Literal("b")]
    parse("return `a{x}b`", |statements, interner| {
        let Expression::InterpolatedString { parts, .. } = return_value(statements) else {
            panic!("expected an interpolated string");
        };
        assert_eq!(parts.len(), 3);
        match &parts[0] {
            InterpolationPart::Literal { value, .. } => assert_eq!(interner.resolve(*value), "a"),
            other => panic!("expected literal, got {other:?}"),
        }
        match &parts[1] {
            InterpolationPart::Expression(Expression::Identifier { name, .. }) => {
                assert_eq!(interner.resolve(*name), "x");
            }
            other => panic!("expected identifier hole, got {other:?}"),
        }
        match &parts[2] {
            InterpolationPart::Literal { value, .. } => assert_eq!(interner.resolve(*value), "b"),
            other => panic!("expected literal, got {other:?}"),
        }
    });
}

#[test]
fn string_interpolation_hole_spans_are_absolute() {
    let source = "return `v={value}`";
    parse(source, |statements, _| {
        let Expression::InterpolatedString { parts, .. } = return_value(statements) else {
            panic!("expected an interpolated string");
        };
        let InterpolationPart::Expression(expr) = &parts[1] else {
            panic!("expected an expression hole");
        };
        let span = expr.span();
        // The hole's span must point exactly at `value` in the original source.
        assert_eq!(&source[span.start as usize..span.end as usize], "value");
    });
}

#[test]
fn type_alias() {
    parse("type Id = number", |statements, interner| {
        let Statement::TypeAlias {
            export,
            symbol,
            value,
            ..
        } = &statements[0]
        else {
            panic!("expected a type alias, got {:?}", statements[0]);
        };
        assert!(!export);
        assert_eq!(interner.resolve(*symbol), "Id");
        let TypeAnnotation::Named { name, .. } = value else {
            panic!("expected a named type");
        };
        assert_eq!(interner.resolve(*name), "number");
    });
}

#[test]
fn export_type_alias_with_generics() {
    parse("export type Box<T> = T", |statements, interner| {
        let Statement::TypeAlias {
            export,
            symbol,
            type_parameters,
            ..
        } = &statements[0]
        else {
            panic!("expected a type alias");
        };
        assert!(export);
        assert_eq!(interner.resolve(*symbol), "Box");
        assert_eq!(type_parameters.len(), 1);
        assert_eq!(interner.resolve(type_parameters[0].symbol), "T");
        assert!(!type_parameters[0].is_pack);
    });
}

#[test]
fn type_is_contextual_identifier() {
    // `type` as a plain local variable name (not the alias keyword).
    parse("local type = 5", |statements, interner| {
        let Statement::Local { variables, .. } = &statements[0] else {
            panic!("expected a local");
        };
        assert_eq!(interner.resolve(variables[0].symbol), "type");
    });
}

#[test]
fn function_type() {
    // Real Luau function-type syntax: `(params) -> returns`.
    parse("type T = (number) -> string", |statements, interner| {
        let Statement::TypeAlias { value, .. } = &statements[0] else {
            panic!("expected a type alias");
        };
        let TypeAnnotation::Function {
            parameters,
            returns,
            ..
        } = value
        else {
            panic!("expected a function type, got {value:?}");
        };
        assert_eq!(parameters.len(), 1);
        let TypeAnnotation::Named { name, .. } = &parameters[0].type_annotation else {
            panic!("expected a named parameter type");
        };
        assert_eq!(interner.resolve(*name), "number");
        assert_eq!(returns.len(), 1);
    });
}

#[test]
fn function_type_with_empty_returns_spans_the_whole_construct() {
    // Regression: `parse_luau_paren_type` fell back to the parameter list's
    // own `)` when the return list was empty, so this span used to stop at
    // `string)`, short of the return `()` entirely.
    let source = "type T = (name: string) -> ()";
    parse(source, |statements, _| {
        let Statement::TypeAlias { value, .. } = &statements[0] else {
            panic!("expected a type alias");
        };
        assert!(matches!(value, TypeAnnotation::Function { .. }));
        let span = value.span();
        assert_eq!(
            &source[span.start as usize..span.end as usize],
            "(name: string) -> ()"
        );
    });
}

#[test]
fn teal_function_type_syntax_is_rejected() {
    // `function(params): returns` is Teal's spelling, not Luau's — Luau
    // writes `(params) -> returns` instead (see `function_type` above).
    common::parse_error("type T = function(number): string", Dialect::Luau);
}

#[test]
fn intersection_type() {
    parse("type T = A & B", |statements, _| {
        let Statement::TypeAlias { value, .. } = &statements[0] else {
            panic!("expected a type alias");
        };
        let TypeAnnotation::Intersection { parts, .. } = value else {
            panic!("expected an intersection type, got {value:?}");
        };
        assert_eq!(parts.len(), 2);
    });
}

#[test]
fn typeof_type() {
    parse("type T = typeof(x)", |statements, _| {
        let Statement::TypeAlias { value, .. } = &statements[0] else {
            panic!("expected a type alias");
        };
        assert!(matches!(value, TypeAnnotation::Typeof { .. }));
    });
}

#[test]
fn singleton_string_type() {
    parse("type Mode = \"read\"", |statements, interner| {
        let Statement::TypeAlias { value, .. } = &statements[0] else {
            panic!("expected a type alias");
        };
        let TypeAnnotation::Singleton {
            value: SingletonValue::String(sym),
            ..
        } = value
        else {
            panic!("expected a string singleton, got {value:?}");
        };
        assert_eq!(interner.resolve(*sym), "\"read\"");
    });
}

#[test]
fn optional_type_span_includes_the_grouping_parentheses() {
    // Regression: a grouped type unwraps transparently in
    // `parse_luau_paren_type` (its `(`/`)` merge into the caller's frame
    // rather than living on a node), so `Optional`'s span used to start from
    // its inner type's own span — `number`, not the `(` — and sliced as
    // `number)?`.
    let source = "type T = (number)?";
    parse(source, |statements, _| {
        let Statement::TypeAlias { value, .. } = &statements[0] else {
            panic!("expected a type alias");
        };
        assert!(matches!(value, TypeAnnotation::Optional { .. }));
        let span = value.span();
        assert_eq!(&source[span.start as usize..span.end as usize], "(number)?");
    });
}

#[test]
fn realistic_multi_statement_program() {
    // Exercises the contextual-keyword interception running across a sequence
    // of statements, plus compound assignment / continue / if-expression /
    // interpolation nested inside.
    let source = "\
type Id = number

local function tally(n: number): number
  local total = 0
  for i = 1, n do
    if i then continue end
    total += i
  end
  return total
end

local msg = `total: {tally(3)}`

local mode = if msg then 1 else 2";
    parse(source, |statements, _| {
        assert_eq!(statements.len(), 4, "four top-level statements");
        assert!(matches!(statements[0], Statement::TypeAlias { .. }));
        assert!(matches!(statements[1], Statement::LocalFunction { .. }));
        assert!(matches!(statements[2], Statement::Local { .. }));
        assert!(matches!(statements[3], Statement::Local { .. }));

        // Inside tally: local, for, return.
        let Statement::LocalFunction { body, .. } = &statements[1] else {
            panic!("expected local function");
        };
        assert_eq!(body.body.len(), 3);
        let Statement::NumericFor { body: for_body, .. } = &body.body[1] else {
            panic!("expected numeric for");
        };
        assert_eq!(for_body.len(), 2);
        assert!(matches!(for_body[0], Statement::If { .. }));
        assert!(matches!(for_body[1], Statement::CompoundAssign { .. }));

        // The interpolated initializer of `msg`.
        let Statement::Local { values, .. } = &statements[2] else {
            panic!("expected local");
        };
        assert!(matches!(values[0], Expression::InterpolatedString { .. }));

        // The if-expression initializer of `mode`.
        let Statement::Local { values, .. } = &statements[3] else {
            panic!("expected local");
        };
        assert!(matches!(values[0], Expression::IfElse { .. }));
    });
}

#[test]
fn typed_parameter_uses_shared_types_machinery() {
    // The shared `: type` annotation machinery is available in Luau.
    parse(
        "local function f(a: number) return a end",
        |statements, interner| {
            let Statement::LocalFunction { body, .. } = &statements[0] else {
                panic!("expected a local function");
            };
            assert_eq!(body.parameters.len(), 1);
            let annotation = body.parameters[0]
                .type_annotation
                .as_ref()
                .expect("typed parameter");
            let TypeAnnotation::Named { name, .. } = annotation else {
                panic!("expected a named type");
            };
            assert_eq!(interner.resolve(*name), "number");
        },
    );
}

// ---- the `f` / newline / `(x)` ambiguity ----
//
// eclipsed splits the statement for a *nested* operand. This is deliberately
// more permissive than real Luau, which rejects `f` newline `(` outright in
// every position (verified against Luau 728) — obfuscator output depends on the
// split, so eclipsed accepts what Luau refuses. Vanilla Lua and full-moon are
// greedy in both positions. See `Dialect::breaks_ambiguous_call`.

#[test]
fn nested_operand_stops_before_a_call_on_the_next_line() {
    // From the Luarmor sample: `c += R` must not become `c += R(C)[U]`, or the
    // `=` on the next line has nowhere to go.
    let source = "c += R\n(C)[U] = c";
    parse(source, |statements, _| {
        assert_eq!(statements.len(), 2, "got {statements:#?}");

        let Statement::CompoundAssign { value, .. } = &statements[0] else {
            panic!("expected `c += R`, got {:?}", statements[0]);
        };
        assert!(
            matches!(value, Expression::Identifier { .. }),
            "the `+=` value must stop at `R`, got {value:?}"
        );

        let Statement::Assign { targets, .. } = &statements[1] else {
            panic!("expected `(C)[U] = c`, got {:?}", statements[1]);
        };
        assert_eq!(targets.len(), 1);
        assert!(matches!(targets[0], Expression::Index { .. }));
    });
}

#[test]
fn assignment_values_stop_before_a_call_on_the_next_line() {
    parse("x = f\n(C)[U] = 1", |statements, _| {
        assert_eq!(statements.len(), 2, "got {statements:#?}");
        let Statement::Assign { values, .. } = &statements[0] else {
            panic!("expected `x = f`, got {:?}", statements[0]);
        };
        assert!(matches!(values[0], Expression::Identifier { .. }));
    });
}

#[test]
fn statement_position_still_reads_a_call_across_a_newline() {
    // Keeping the split to nested operands means a deliberate call written at
    // statement level is still read as a call, rather than silently becoming two
    // statements. (Real Luau rejects this form; eclipsed is permissive here.)
    parse("f\n(x)", |statements, _| {
        assert_eq!(statements.len(), 1, "got {statements:#?}");
        let Statement::Expression { expr, .. } = &statements[0] else {
            panic!("expected a call statement, got {:?}", statements[0]);
        };
        assert!(matches!(**expr, Expression::Call { .. }));
    });
}

#[test]
fn a_broken_off_parenthesis_must_still_form_a_statement() {
    // The flip side of the rule: having stopped at `f`, the `(y)` line has to
    // stand on its own, and a bare parenthesised value is not a statement.
    common::parse_error("local x = f\n(y)", Dialect::Luau);
}

#[test]
fn a_call_on_the_same_line_is_unaffected() {
    parse("x = f(y)", |statements, _| {
        let Statement::Assign { values, .. } = &statements[0] else {
            panic!("expected an assignment, got {:?}", statements[0]);
        };
        assert!(matches!(values[0], Expression::Call { .. }));
    });
}

#[test]
fn interpolation_hole_must_be_one_expression() {
    common::parse_error("return `{a b}`", Dialect::Luau);
    common::parse_error("return `{}`", Dialect::Luau);
}

#[test]
fn deeply_nested_types_are_an_error() {
    let depth = 50_000;
    let source = format!("type T = {}number{}", "{".repeat(depth), "}".repeat(depth));
    assert!(common::parse_error(&source, Dialect::Luau).contains("nesting"));
}

#[test]
fn failed_instantiations_are_not_retried() {
    let depth = 40;
    for (open, close) in [("a<<typeof(", ")>>"), ("a<<typeof(`{", "}`)>>")] {
        let source = format!("return {}a{}", open.repeat(depth), close.repeat(depth));
        let started = std::time::Instant::now();
        let arena = bumpalo::Bump::new();
        let tokens = eclipsed::lexer::Lexer::new(&source, &arena, Dialect::Luau).lex();
        let mut interner = eclipsed::interner::Interner::new();
        let _ =
            eclipsed::parser::Parser::new(tokens, &source, &arena, &mut interner, Dialect::Luau)
                .parse();
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }
}

#[test]
fn a_half_consumed_shift_is_a_comparison() {
    parse("local x = y :: A<B>> 1", |statements, _| {
        let Statement::Local { values, .. } = &statements[0] else {
            panic!("expected a local, got {:?}", statements[0]);
        };
        let Expression::Binary { operator, .. } = &values[0] else {
            panic!("expected a binary operation, got {:?}", values[0]);
        };
        assert_eq!(*operator, BinaryOperator::GreaterThan);
    });
}

#[test]
fn type_packs_are_only_allowed_at_the_top_of_a_return_type() {
    parse("function f(): (string, boolean) end", |_, _| {});
    common::parse_error(
        "function f(): number | (string, boolean) end",
        Dialect::Luau,
    );
    common::parse_error("function f(): { x: (string, boolean) } end", Dialect::Luau);
}

#[test]
fn attributes_do_not_attach_to_an_initializer() {
    parse("@native local function f() end", |_, _| {});
    common::parse_error("@native local x = function() end", Dialect::Luau);
}

#[test]
fn unicode_escapes_in_interpolation_are_not_holes() {
    parse("return `\\u{48}`", |statements, _| {
        let Expression::InterpolatedString { parts, .. } = return_value(statements) else {
            panic!("expected an interpolated string");
        };
        assert_eq!(parts.len(), 1);
        assert!(matches!(parts[0], InterpolationPart::Literal { .. }));
    });
    parse("return `a\\u{7B}b{c}`", |statements, _| {
        let Expression::InterpolatedString { parts, .. } = return_value(statements) else {
            panic!("expected an interpolated string");
        };
        assert_eq!(parts.len(), 2);
    });
}

#[test]
fn braces_inside_hole_strings_and_comments_do_not_close_it() {
    for source in [
        "return `{ \"}\" }`",
        "return `{ [[}]] }`",
        "return `{ `}` }`",
        "return `{x --[[ } ]] }`",
    ] {
        parse(source, |statements, _| {
            let Expression::InterpolatedString { parts, .. } = return_value(statements) else {
                panic!("expected an interpolated string for {source:?}");
            };
            assert_eq!(parts.len(), 1, "{source:?}");
            assert!(
                matches!(parts[0], InterpolationPart::Expression(_)),
                "{source:?}"
            );
        });
    }
}

#[test]
fn lexing_an_inverted_range_is_empty() {
    let arena = bumpalo::Bump::new();
    let tokens = eclipsed::lexer::Lexer::lex_range("abc", &arena, Dialect::Luau, 3, 1);
    assert!(tokens.is_empty());
}

#[test]
fn spans_include_the_grouping_parentheses_of_their_types() {
    fn alias_span(source: &str) -> &str {
        parse_with(source, Dialect::Luau, |statements, _| {
            let Statement::TypeAlias { value, .. } = &statements[0] else {
                panic!("expected a type alias");
            };
            let span = value.span();
            &source[span.start as usize..span.end as usize]
        })
    }
    assert_eq!(alias_span("type T = (A) | (B)"), "(A) | (B)");
    assert_eq!(alias_span("type T = | A | B"), "| A | B");
    assert_eq!(alias_span("type T = (A) & (B)"), "(A) & (B)");

    let source = "return y :: (number)";
    parse(source, |statements, _| {
        let span = return_value(statements).span();
        assert_eq!(
            &source[span.start as usize..span.end as usize],
            "y :: (number)"
        );
    });
    let source = "local x = y :: A<B>> 1";
    parse(source, |statements, _| {
        let Statement::Local { values, .. } = &statements[0] else {
            panic!("expected a local");
        };
        let Expression::Binary { left, .. } = &values[0] else {
            panic!("expected a binary operation");
        };
        let span = left.span();
        assert_eq!(&source[span.start as usize..span.end as usize], "y :: A<B>");
    });
}

#[test]
fn deeply_nested_interpolation_does_not_overflow_the_lexer() {
    let source = format!("return {}", "`{".repeat(100_000));
    let arena = bumpalo::Bump::new();
    let tokens = eclipsed::lexer::Lexer::new(&source, &arena, Dialect::Luau).lex();
    assert_eq!(
        tokens.last().unwrap().token_type,
        eclipsed::lexer::Type::UnterminatedString
    );
}

#[test]
fn an_instantiation_may_close_on_a_split_shift() {
    for source in [
        "return f<<A<B>>>()",
        "return f<<A<B> >>()",
        "return f<<A<B<C>>>>()",
        "return f<<A<B<C<D>>>>>()",
    ] {
        parse(source, |statements, _| {
            let Expression::Call { arguments, .. } = return_value(statements) else {
                panic!("expected a call for {source:?}");
            };
            let CallArguments::Expressions { type_arguments, .. } = arguments else {
                panic!("expected an argument list for {source:?}");
            };
            assert_eq!(type_arguments.map(<[_]>::len), Some(1), "{source:?}");
        });
    }
    common::parse_error("return f<<A<B> > >()", Dialect::Luau);
    common::parse_error("return f<<A<B>> >()", Dialect::Luau);
}
