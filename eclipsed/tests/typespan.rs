//! Where the parser says Luau type syntax lives.
//!
//! Each case asserts the *text* the reported spans cover, not their offsets:
//! that is what a caller deleting them actually sees, and it keeps the test
//! readable when the sample moves.
#![cfg(all(feature = "cst", feature = "luau"))]

use eclipsed::api;
use eclipsed::dialect::Dialect;
use eclipsed::typespan::TypeSpanKind;

/// Every reported span as `(kind, covered text)`, in source order.
fn spans(source: &str) -> Vec<(TypeSpanKind, String)> {
    api::with_chunk(source, Dialect::Luau, |chunk| {
        chunk
            .type_spans()
            .into_iter()
            .map(|entry| (entry.kind, chunk.text(entry.span).to_string()))
            .collect()
    })
    .expect("sample parses")
}

fn assert_spans(source: &str, expected: &[(TypeSpanKind, &str)]) {
    let found = spans(source);
    let found: Vec<(TypeSpanKind, &str)> = found
        .iter()
        .map(|(kind, text)| (*kind, text.as_str()))
        .collect();
    assert_eq!(found, expected, "for source:\n{source}");
}

#[test]
fn a_small_luau_sample() {
    // Parameter annotations, a generic parameter list, a parenthesized return
    // list, a nested closure, a cast and a type alias — one of each thing the
    // walk has to find.
    let source = "\
type Id = number
export type Pair<K, V> = { first: K, second: V }

local count: number = 0

function Registry.Add<Item>(item: Item, key: Id?, ...: any): (boolean, string)
\tlocal cached: { [string]: Item } = {}
\tlocal handler = function(value: Item): Item?
\t\treturn value
\tend
\tfor index: number, entry: Item in pairs(cached) do
\t\tcount += index
\tend
\treturn true, tostring(handler(item)) :: string
end
";

    use TypeSpanKind::{AliasStatement, Annotation, Parameter};
    assert_spans(
        source,
        &[
            (AliasStatement, "type Id = number"),
            (
                AliasStatement,
                "export type Pair<K, V> = { first: K, second: V }",
            ),
            (Annotation, "number"),
            (Parameter, "Item"),
            (Annotation, "Item"),
            (Annotation, "Id?"),
            (Annotation, "any"),
            (Annotation, "(boolean, string)"),
            (Annotation, "{ [string]: Item }"),
            (Annotation, "Item"),
            (Annotation, "Item?"),
            (Annotation, "number"),
            (Annotation, "Item"),
            (Annotation, "string"),
        ],
    );
}

#[test]
fn annotations_on_if_bindings_are_reported() {
    use TypeSpanKind::Annotation;
    assert_spans(
        "if local x: number? = get() then end\nlocal y = if const z: string = get() then z else \"\"",
        &[(Annotation, "number?"), (Annotation, "string")],
    );
}

#[test]
fn an_alias_reports_only_itself() {
    // The inner `K`, the struct field and the generic parameters are all
    // inside the statement's range; reporting them again would be redundant.
    use TypeSpanKind::AliasStatement;
    assert_spans(
        "type Box<T> = { value: T, next: Box<Box<T>>? }",
        &[(
            AliasStatement,
            "type Box<T> = { value: T, next: Box<Box<T>>? }",
        )],
    );
}

#[test]
fn a_nested_generic_span_ends_on_a_token_boundary() {
    // `Iter<Pair<K, V>>` closes on a single `>>` token. The reported span must
    // cover all of it, or a caller dropping "the tokens inside" would leave a
    // stray `>`.
    use TypeSpanKind::{Annotation, Parameter};
    assert_spans(
        "function Iter:Zip<K, V>(other: Iter<V>): Iter<Pair<K, V>>\n\treturn other\nend\n",
        &[
            (Parameter, "K"),
            (Parameter, "V"),
            (Annotation, "Iter<V>"),
            (Annotation, "Iter<Pair<K, V>>"),
        ],
    );
}

#[test]
fn an_empty_return_list_is_found_without_an_annotation() {
    // `: ()` parses to an empty slice of annotations, so there is nothing in
    // the AST to point at; the range comes from the tokens instead.
    use TypeSpanKind::{Annotation, Parameter};
    assert_spans("function f(): ()\nend\n", &[(Annotation, "()")]);
    assert_spans(
        "function f<T>(a: T, ...: any): ()\nend\n",
        &[
            (Parameter, "T"),
            (Annotation, "T"),
            (Annotation, "any"),
            (Annotation, "()"),
        ],
    );
}

#[test]
fn the_walk_reaches_into_nested_scopes() {
    // A bundler wraps every module in `modules[path] = function() ... end`, so
    // a top-level-only pass would find nothing at all in a bundle.
    use TypeSpanKind::Annotation;
    let source = "\
modules[\"a\"] = function()
\tdo
\t\tif true then
\t\t\tlocal t = { f = function(x: number) return x end }
\t\tend
\tend
end
";
    assert_spans(source, &[(Annotation, "number")]);
}

#[test]
fn a_vararg_pack_and_a_function_type_parameter_are_reported() {
    use TypeSpanKind::{Annotation, Parameter};
    assert_spans(
        "function Try<A..., R...>(fn: (A...) -> R..., ...: A...): (boolean, any)\nend\n",
        &[
            (Parameter, "A"),
            (Parameter, "R"),
            (Annotation, "(A...) -> R..."),
            (Annotation, "A..."),
            (Annotation, "(boolean, any)"),
        ],
    );
}

#[test]
fn a_type_function_is_reported_as_one_statement() {
    use TypeSpanKind::AliasStatement;
    assert_spans(
        "type function reverse(t)\n\treturn t\nend\n",
        &[(AliasStatement, "type function reverse(t)\n\treturn t\nend")],
    );
}

#[test]
fn plain_lua_has_nothing_to_report() {
    assert_spans("local a = 1\nfor i = 1, 10 do print(i) end\n", &[]);
}
