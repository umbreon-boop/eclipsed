//! Trivia classification and the leading/trailing attachment rule.
#![cfg(feature = "cst")]

use eclipsed::cst::{Trivia, TriviaKind};
use eclipsed::lexer::Span;

#[test]
fn trivia_carries_kind_and_span() {
    let trivia = Trivia {
        kind: TriviaKind::LineComment,
        span: Span::new(0, 7),
    };
    assert_eq!(trivia.span.start, 0);
    assert_eq!(trivia.span.end, 7);
    assert!(matches!(trivia.kind, TriviaKind::LineComment));
}

use bumpalo::Bump;
use eclipsed::dialect::Dialect;
use eclipsed::lexer::Lexer;
use eclipsed::stream::TokenStream;

/// Concatenating every token's leading trivia, text, and trailing trivia, in
/// order, must reproduce the source exactly. This is lexer-level losslessness,
/// independent of the parser.
///
/// Delegates to [`TokenStream::render`] rather than reimplementing the
/// concatenation here: a hand-rolled copy could drift from the shipped printer
/// and pass while `render` itself was broken.
fn assert_lexer_lossless(source: &str, dialect: Dialect) {
    let out = TokenStream::lex(source, dialect).render();
    assert_eq!(out, source, "lexer round-trip differed");
}

#[test]
fn line_comment_after_code_is_trailing() {
    let source = "local x = 1 -- why\nlocal y = 2\n";
    let arena = Bump::new();
    let tokens = Lexer::new(source, &arena, Dialect::Lua).lex_cst();

    // `1` owns the comment: it sits before the newline that ends the line.
    let one = tokens.iter().find(|t| t.text(source) == "1").unwrap();
    assert!(
        one.trailing
            .iter()
            .any(|t| t.kind == TriviaKind::LineComment),
        "comment should be trailing trivia of `1`, got {:?}",
        one.trailing
    );
}

#[test]
fn comment_on_its_own_line_is_leading_of_next_token() {
    let source = "local x = 1\n-- standalone\nlocal y = 2\n";
    let arena = Bump::new();
    let tokens = Lexer::new(source, &arena, Dialect::Lua).lex_cst();

    // The second `local` owns it: the comment follows a newline.
    let second_local = tokens
        .iter()
        .filter(|t| t.text(source) == "local")
        .nth(1)
        .unwrap();
    assert!(
        second_local
            .leading
            .iter()
            .any(|t| t.kind == TriviaKind::LineComment),
        "comment should be leading trivia of the second `local`, got {:?}",
        second_local.leading
    );
}

#[test]
fn whitespace_never_spans_a_newline() {
    let source = "local x = 1\n\n\nlocal y = 2\n";
    let arena = Bump::new();
    let tokens = Lexer::new(source, &arena, Dialect::Lua).lex_cst();

    for token in tokens {
        for trivia in token.leading.iter().chain(token.trailing) {
            if trivia.kind == TriviaKind::Whitespace {
                let text = &source[trivia.span.start as usize..trivia.span.end as usize];
                assert!(
                    !text.contains('\n'),
                    "whitespace trivia contained a newline"
                );
            }
        }
    }
}

#[test]
fn eof_token_carries_trailing_trivia() {
    // A file ending in a comment cannot round-trip without an EOF token.
    let source = "local x = 1\n-- last word\n";
    let arena = Bump::new();
    let tokens = Lexer::new(source, &arena, Dialect::Lua).lex_cst();

    let eof = tokens.last().unwrap();
    assert_eq!(eof.token_type, eclipsed::lexer::Type::EndOfFile);
    assert!(
        eof.leading
            .iter()
            .any(|t| t.kind == TriviaKind::LineComment),
        "trailing comment should reach the EOF token"
    );
    assert_lexer_lossless(source, Dialect::Lua);
}

#[test]
fn shebang_becomes_leading_trivia() {
    let source = "#!/usr/bin/env lua\nlocal x = 1\n";
    let arena = Bump::new();
    let tokens = Lexer::new(source, &arena, Dialect::Lua).lex_cst();

    assert!(
        tokens[0]
            .leading
            .iter()
            .any(|t| t.kind == TriviaKind::Shebang),
        "shebang should be leading trivia of the first token"
    );
    assert_lexer_lossless(source, Dialect::Lua);
}

#[test]
fn long_comments_round_trip() {
    let source = "local x = 1 --[==[ multi\nline ]==] local y = 2\n";
    assert_lexer_lossless(source, Dialect::Lua);

    let arena = Bump::new();
    let tokens = Lexer::new(source, &arena, Dialect::Lua).lex_cst();
    let found = tokens
        .iter()
        .flat_map(|t| t.leading.iter().chain(t.trailing))
        .any(|t| {
            t.kind == TriviaKind::LongComment
                && &source[t.span.range()] == "--[==[ multi\nline ]==]"
        });
    assert!(found, "expected the long comment as one trivia");
}

#[test]
fn lexer_is_lossless_on_assorted_sources() {
    for source in [
        "",
        "\n",
        "   \n\n  ",
        "-- only a comment",
        "local x = 1;;; local y = 2",
        "local t = {a, b; c}",
        "return 1 --[[inline]] + 2",
    ] {
        assert_lexer_lossless(source, Dialect::Lua);
    }
}

#[cfg(feature = "cfxlua")]
#[test]
fn c_style_comments_round_trip() {
    assert_lexer_lossless("local x = 1 /* c style */ local y = 2\n", Dialect::CfxLua);
}

/// `TokenRef::full_span` takes `leading.first()` and `trailing.last()` rather
/// than scanning, so it is only correct while each slice is sorted ascending
/// and contiguous with the token. Nothing else checks this, and a builder that
/// pushed trivia out of order would make `full_span` silently wrong.
#[test]
fn trivia_slices_are_ordered_and_contiguous() {
    let source = "  -- lead\nlocal x = 1 -- trail\n\nlocal y = 2\n";
    let arena = Bump::new();
    let tokens = Lexer::new(source, &arena, Dialect::Lua).lex_cst();

    for token in tokens {
        for slice in [token.leading, token.trailing] {
            for pair in slice.windows(2) {
                assert!(
                    pair[0].span.end <= pair[1].span.start,
                    "trivia out of order: {:?} then {:?}",
                    pair[0],
                    pair[1]
                );
            }
        }
        if let Some(last) = token.leading.last() {
            assert_eq!(
                last.span.end, token.span.start,
                "leading must abut the token"
            );
        }
        if let Some(first) = token.trailing.first() {
            assert_eq!(
                token.span.end, first.span.start,
                "trailing must abut the token"
            );
        }

        let full = token.full_span();
        assert!(full.start <= token.span.start && full.end >= token.span.end);
    }
}

// --- CRLF / lone-CR line endings ---------------------------------------
//
// None of the 287 corpus files exercised by the lexer round-trip test
// contain a carriage return, so `collect_whitespace`'s `\r` / `\r\n` branch
// (and the line-comment / shebang scanning that decides where a line ends)
// had never actually run under a test. Windows-authored (`\r\n`) and classic
// Mac-authored (lone `\r`) Lua files are common enough that this needed
// direct coverage.

#[test]
fn crlf_line_endings_round_trip() {
    let source = "local x = 1\r\nlocal y = 2\r\nreturn x + y\r\n";
    assert_lexer_lossless(source, Dialect::Lua);
}

#[test]
fn lone_cr_line_endings_round_trip() {
    let source = "local x = 1\rlocal y = 2\rreturn x + y\r";
    assert_lexer_lossless(source, Dialect::Lua);
}

#[test]
fn mixed_line_endings_round_trip() {
    let source = "local x = 1\nlocal y = 2\r\nlocal z = 3\rreturn x + y + z\n";
    assert_lexer_lossless(source, Dialect::Lua);
}

/// This is where the trailing/leading trivia split is decided: `next_token_cst`
/// partitions the run of collected trivia at the first `Newline`. If a line
/// comment's own scanner does not stop at `\r`, the `\r` half of a CRLF pair
/// gets swallowed into the comment instead of starting the newline — which,
/// left unbounded (no `\n` anywhere else in the file), can swallow the rest
/// of the source into the "comment".
#[test]
fn crlf_immediately_after_line_comment_round_trips() {
    let source = "-- c\r\nlocal x = 1\r\n";
    assert_lexer_lossless(source, Dialect::Lua);

    let arena = Bump::new();
    let tokens = Lexer::new(source, &arena, Dialect::Lua).lex_cst();
    let local = tokens.iter().find(|t| t.text(source) == "local").unwrap();

    let comment = local
        .leading
        .iter()
        .find(|t| t.kind == TriviaKind::LineComment)
        .expect("line comment should be leading trivia of `local`");
    assert_eq!(
        &source[comment.span.start as usize..comment.span.end as usize],
        "-- c",
        "the comment must not swallow the `\\r` that starts the line ending"
    );

    let newline_count = local
        .leading
        .iter()
        .filter(|t| t.kind == TriviaKind::Newline)
        .count();
    assert_eq!(
        newline_count, 1,
        "a `\\r\\n` pair must be exactly one Newline trivium, got {:?}",
        local.leading
    );
}

/// The catastrophic case: a line comment terminated by a lone `\r` with no
/// other `\n` anywhere in the file. Before the `lex_comment` fix, the scanner
/// only stopped at `\n`, so it never found an end to the comment and the
/// entire rest of the source — including the `local` statement — was
/// swallowed into a single `LineComment` trivium, leaving zero real tokens.
#[test]
fn lone_cr_immediately_after_line_comment_round_trips() {
    let source = "-- c\rlocal x = 1\r";
    assert_lexer_lossless(source, Dialect::Lua);

    let arena = Bump::new();
    let tokens = Lexer::new(source, &arena, Dialect::Lua).lex_cst();

    let local = tokens
        .iter()
        .find(|t| t.text(source) == "local")
        .expect("`local` must survive as a real token, not be swallowed into the comment");

    let comment = local
        .leading
        .iter()
        .find(|t| t.kind == TriviaKind::LineComment)
        .expect("line comment should be leading trivia of `local`");
    assert_eq!(
        &source[comment.span.start as usize..comment.span.end as usize],
        "-- c",
        "the comment must not swallow the `\\r` that ends the line"
    );

    let newline_count = local
        .leading
        .iter()
        .filter(|t| t.kind == TriviaKind::Newline)
        .count();
    assert_eq!(
        newline_count, 1,
        "the lone `\\r` must be exactly one Newline trivium, got {:?}",
        local.leading
    );
}

#[test]
fn crlf_inside_long_comment_round_trips() {
    let source = "local a = 1 --[[ a\r\nb ]] local x = 1\r\n";
    assert_lexer_lossless(source, Dialect::Lua);
}

#[test]
fn file_ending_in_crlf_with_nothing_after_round_trips() {
    let source = "local x = 1\r\n";
    assert_lexer_lossless(source, Dialect::Lua);

    let arena = Bump::new();
    let tokens = Lexer::new(source, &arena, Dialect::Lua).lex_cst();
    assert_eq!(
        tokens.last().unwrap().token_type,
        eclipsed::lexer::Type::EndOfFile
    );
}

/// A shebang is skipped before lexing proper begins ([`Lexer::new`]), by a
/// scanner with the same "stop at line end" job as the line-comment scanner
/// above — and the same failure mode: stopping on `\n` alone lets a lone `\r`
/// pass through as shebang content, which (with no other `\n` in the file)
/// can swallow the entire source into the shebang.
#[test]
fn crlf_around_shebang_round_trips() {
    let source = "#!/usr/bin/env lua\r\nlocal x = 1\r\n";
    assert_lexer_lossless(source, Dialect::Lua);

    let arena = Bump::new();
    let tokens = Lexer::new(source, &arena, Dialect::Lua).lex_cst();
    let shebang = tokens[0]
        .leading
        .iter()
        .find(|t| t.kind == TriviaKind::Shebang)
        .expect("shebang should be leading trivia of the first token");
    assert_eq!(
        &source[shebang.span.start as usize..shebang.span.end as usize],
        "#!/usr/bin/env lua",
        "the shebang must not swallow the `\\r` that starts the line ending"
    );
    assert!(
        tokens.iter().any(|t| t.text(source) == "local"),
        "`local` must survive as a real token"
    );
}

/// The catastrophic case: a shebang line ended by a lone `\r` with no other
/// `\n` anywhere in the file. Before the `Lexer::new` fix, the skip loop only
/// stopped at `\n`, so it never found an end to the shebang and swallowed the
/// entire rest of the source into it, leaving nothing but an `EndOfFile`
/// token.
#[test]
fn lone_cr_around_shebang_round_trips() {
    let source = "#!/usr/bin/env lua\rlocal x = 1\r";
    assert_lexer_lossless(source, Dialect::Lua);

    let arena = Bump::new();
    let tokens = Lexer::new(source, &arena, Dialect::Lua).lex_cst();
    let shebang = tokens[0]
        .leading
        .iter()
        .find(|t| t.kind == TriviaKind::Shebang)
        .expect("shebang should be leading trivia of the first token");
    assert_eq!(
        &source[shebang.span.start as usize..shebang.span.end as usize],
        "#!/usr/bin/env lua",
        "the shebang must not swallow the `\\r` that ends the line"
    );
    assert!(
        tokens
            .iter()
            .any(|t| t.token_type != eclipsed::lexer::Type::EndOfFile),
        "the file must not have been swallowed entirely into the shebang"
    );
    assert!(
        tokens.iter().any(|t| t.text(source) == "local"),
        "`local` must survive as a real token, not be swallowed into the shebang"
    );
}

#[test]
fn crlf_pair_is_exactly_one_newline_trivium() {
    let source = "local x = 1\r\nlocal y = 2\n";
    let arena = Bump::new();
    let tokens = Lexer::new(source, &arena, Dialect::Lua).lex_cst();

    let one = tokens.iter().find(|t| t.text(source) == "1").unwrap();
    let newlines: Vec<_> = one
        .trailing
        .iter()
        .filter(|t| t.kind == TriviaKind::Newline)
        .collect();
    assert_eq!(
        newlines.len(),
        1,
        "a `\\r\\n` pair must produce exactly one Newline trivium (not two, and not \
         Whitespace + Newline), got {:?}",
        one.trailing
    );
    assert_eq!(
        &source[newlines[0].span.start as usize..newlines[0].span.end as usize],
        "\r\n"
    );
    assert!(
        !one.trailing
            .iter()
            .any(|t| t.kind == TriviaKind::Whitespace),
        "no separate Whitespace trivium should appear for the `\\r`, got {:?}",
        one.trailing
    );
}

#[test]
fn lone_cr_is_exactly_one_newline_trivium() {
    let source = "local x = 1\rlocal y = 2\n";
    let arena = Bump::new();
    let tokens = Lexer::new(source, &arena, Dialect::Lua).lex_cst();

    let one = tokens.iter().find(|t| t.text(source) == "1").unwrap();
    let newlines: Vec<_> = one
        .trailing
        .iter()
        .filter(|t| t.kind == TriviaKind::Newline)
        .collect();
    assert_eq!(
        newlines.len(),
        1,
        "a lone `\\r` must produce exactly one Newline trivium, got {:?}",
        one.trailing
    );
    assert_eq!(
        &source[newlines[0].span.start as usize..newlines[0].span.end as usize],
        "\r"
    );
}
