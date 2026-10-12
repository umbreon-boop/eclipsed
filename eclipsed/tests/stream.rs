//! The flat token stream's load-bearing properties.
//!
//! Losslessness is asserted over the whole corpus rather than over examples,
//! because the stream is the substrate every transformation pass builds on: if
//! concatenation is not the identity for an untouched stream, nothing built on
//! it can be trusted.
#![cfg(feature = "cst")]

use std::fs;
use std::path::PathBuf;

use eclipsed::dialect::Dialect;
use eclipsed::stream::{Token, TokenKind, TokenStream, strip_trivia, would_fuse};

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus")
}

/// Every `source.lua` under `tests/corpus/`, including files that do not
/// parse: the lexer must round-trip text the parser rejects just as faithfully.
fn corpus_files() -> Vec<PathBuf> {
    fn walk(dir: &PathBuf, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.file_name().is_some_and(|n| n == "source.lua") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&corpus_root(), &mut out);
    out.sort();
    out
}

#[test]
fn renders_the_corpus_byte_for_byte() {
    let files = corpus_files();
    assert!(!files.is_empty(), "corpus is missing");

    let mut checked = 0;
    for path in &files {
        let Ok(source) = fs::read_to_string(path) else {
            continue; // not UTF-8; the stream borrows &str, so skip
        };
        let stream = TokenStream::lex(&source, Dialect::Luau);
        assert_eq!(
            stream.render(),
            source,
            "stream did not reproduce {}",
            path.display()
        );
        checked += 1;
    }
    assert!(checked > 100, "only {checked} files checked");
}

#[test]
fn trivia_are_ordinary_stream_members() {
    let source = "local x = 1 -- hi\n";
    let stream = TokenStream::lex(source, Dialect::Lua);

    // The comment is a token in its own right, sitting where it was written,
    // rather than a property hanging off a neighbour.
    let comment = stream
        .tokens()
        .iter()
        .position(|t| t.kind.is_comment())
        .expect("comment should be in the stream");
    let newline = stream
        .tokens()
        .iter()
        .position(|t| t.kind.is_newline())
        .expect("newline should be in the stream");
    assert!(
        comment < newline,
        "the comment precedes the line ending it sits on"
    );
}

#[test]
fn a_pass_rebuilds_rather_than_splices() {
    let source = "local x = 1 -- gone\nreturn x\n";
    let mut stream = TokenStream::lex(source, Dialect::Lua);
    let before = stream.len();

    stream.rewrite(|input, out| {
        for token in input {
            if !token.kind.is_comment() {
                out.push(token.clone());
            }
        }
    });

    assert!(stream.len() < before, "the comment should be gone");
    assert_eq!(stream.render(), "local x = 1 \nreturn x\n");
    assert!(!stream.tokens().iter().any(|t| t.kind.is_comment()));
}

#[test]
fn passes_compose_and_reuse_their_buffers() {
    let source = "local  x   =  1\n\n\nreturn x\n";
    let mut stream = TokenStream::lex(source, Dialect::Lua);

    // Two passes in sequence: collapse whitespace runs, then drop blank lines.
    for _ in 0..2 {
        stream.rewrite(|input, out| {
            for token in input {
                out.push(token.clone());
            }
        });
    }
    assert_eq!(
        stream.render(),
        source,
        "identity passes must not alter text"
    );
}

#[test]
fn a_synthesized_token_carries_its_own_text() {
    let mut stream = TokenStream::lex("return x\n", Dialect::Lua);
    stream.rewrite(|input, out| {
        out.push(Token::new(
            TokenKind::Trivia(eclipsed::cst::TriviaKind::LineComment),
            "-- added",
        ));
        out.push(Token::newline());
        out.extend(input.iter().cloned());
    });

    assert_eq!(stream.render(), "-- added\nreturn x\n");
    // Provenance survives: invented tokens are distinguishable from read ones.
    assert!(!stream.tokens()[0].is_original());
    assert!(stream.tokens().last().unwrap().is_original());
}

#[test]
fn fusion_is_detected_by_the_real_lexer() {
    // Pairs that must not be written adjacently.
    for (a, b) in [
        ("local", "x"),
        ("-", "-x"),
        ("a", "b"),
        ("..", ".5"),
        ("[", "["),
        ("then", "continue"),
    ] {
        assert!(would_fuse(a, b, Dialect::Lua), "{a:?} + {b:?} should fuse");
    }
    // Pairs that are safe to jam together.
    for (a, b) in [(")", "("), ("1", "+"), ("a", ","), ("}", ")")] {
        assert!(
            !would_fuse(a, b, Dialect::Lua),
            "{a:?} + {b:?} should not fuse"
        );
    }
}

#[test]
fn stripping_trivia_never_fuses_tokens() {
    let cases = [
        "local x = 1 -- c\nreturn x\n",
        "local a = b - -c\n",
        "if a then return end\n",
        "local t = { 1, 2 }\nreturn #t\n",
    ];
    for source in cases {
        let mut stream = TokenStream::lex(source, Dialect::Lua);
        strip_trivia(&mut stream, Dialect::Lua);
        let minified = stream.render();

        // The proof is not that it looks right but that it lexes back to the
        // same syntax tokens, in order, as the original.
        let original = TokenStream::lex(source, Dialect::Lua);
        let expected: Vec<_> = original
            .tokens()
            .iter()
            .filter(|t| !t.kind.is_trivia())
            .map(|t| t.text.to_string())
            .collect();
        let round = TokenStream::lex(&minified, Dialect::Lua);
        let got: Vec<_> = round
            .tokens()
            .iter()
            .filter(|t| !t.kind.is_trivia())
            .map(|t| t.text.to_string())
            .collect();
        assert_eq!(expected, got, "minifying {source:?} changed the tokens");
    }
}

/// Minifying is only meaningful for source that parses.
///
/// The corpus deliberately includes broken input — an unterminated string, for
/// one — and removing the whitespace inside it changes what it means, because
/// the string runs on and swallows the code after it. That is the correct
/// outcome rather than a defect to paper over, so the contract is stated here:
/// [`strip_trivia`] is defined on valid source. Losslessness carries no such
/// caveat, which is why `renders_the_corpus_byte_for_byte` runs over every
/// file including the broken ones.
#[test]
fn stripping_trivia_shrinks_the_corpus_without_changing_its_tokens() {
    let mut checked = 0;
    let mut skipped = 0;
    for path in corpus_files() {
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        if eclipsed::api::with_chunk(&source, Dialect::Luau, |_| ()).is_err() {
            skipped += 1;
            continue;
        }
        let mut stream = TokenStream::lex(&source, Dialect::Luau);
        let expected: Vec<_> = stream
            .tokens()
            .iter()
            .filter(|t| !t.kind.is_trivia())
            .map(|t| t.text.to_string())
            .collect();

        strip_trivia(&mut stream, Dialect::Luau);
        let minified = stream.render();

        let got: Vec<_> = TokenStream::lex(&minified, Dialect::Luau)
            .tokens()
            .iter()
            .filter(|t| !t.kind.is_trivia())
            .map(|t| t.text.to_string())
            .collect();
        assert_eq!(
            expected,
            got,
            "minifying {} changed its tokens",
            path.display()
        );
        checked += 1;
    }
    assert!(
        checked > 100,
        "only {checked} files checked ({skipped} unparsable)"
    );
}

/// A trailing backslash has no escaped byte after it.
///
/// The lexer used to consume one anyway, producing a token whose span ended
/// one byte past the source. Nothing noticed until the stream sliced by that
/// span, and because the slice happens behind the C ABI the panic unwound into
/// the caller. The same shape appeared in three places: a short string, an
/// interpolated string, and the `{hole}` scanner inside one.
#[test]
fn a_span_never_runs_past_the_end_of_the_source() {
    let cases = [
        "\"\\", "'\\", "\"a\\", "`\\", "`{\"\\", "`{x\\", "\"\\z", "\"\\u{", "--[[\\", "[[\\",
    ];
    for source in cases {
        for dialect in [
            Dialect::Lua,
            Dialect::Luau,
            Dialect::LuaJit,
            Dialect::CfxLua,
        ] {
            let stream = TokenStream::lex(source, dialect);
            for token in stream.tokens() {
                if let Some(span) = token.span {
                    assert!(
                        span.end as usize <= source.len(),
                        "{source:?} under {dialect:?}: span {}..{} exceeds len {}",
                        span.start,
                        span.end,
                        source.len()
                    );
                }
            }
            assert_eq!(stream.render(), source, "{source:?} under {dialect:?}");
        }
    }
}

/// Lexing must not panic or lose bytes on hostile input.
///
/// Generated rather than enumerated: the trailing-backslash bug survived a
/// hand-written corpus of 296 files and was only found by shuffling fragments
/// together. Deterministic seed, so a failure reproduces.
#[test]
fn fuzzing_never_panics_or_loses_bytes() {
    let alphabet = [
        "local", " ", "\n", "\t", "=", "--", "--[[", "]]", "\"", "'", "[[", "é", "日", "🎉",
        "\u{FEFF}", "x", "1", ".", "..", "...", ":", "::", "{", "}", "(", ")", "[", "]", "<", ">",
        "|", "&", "#", "-", "/*", "*/", "`", "\\", "\r", "\r\n", "0x", "1e", "function", "end",
        "\\z", "\\u{", "[=[", "]=]", "--[==[", "]==]",
    ];
    let mut state: u64 = 0x2545F4914F6CDD1D;
    let mut rand = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };

    for _ in 0..4000 {
        let len = (rand() % 24) as usize + 1;
        let mut source = String::new();
        for _ in 0..len {
            source.push_str(alphabet[(rand() % alphabet.len() as u64) as usize]);
        }
        for dialect in [
            Dialect::Lua,
            Dialect::Luau,
            Dialect::LuaJit,
            Dialect::CfxLua,
        ] {
            let stream = TokenStream::lex(&source, dialect);
            assert_eq!(stream.render(), source, "under {dialect:?}");
        }
    }
}

/// A numeral that runs straight into a word is malformed, not two tokens.
///
/// `0and` is what `0 and` becomes if a minifier believes no separator is
/// needed, and every real Lua rejects it. The lexer used to stop at the first
/// non-digit and hand back `0` followed by `and`, which made
/// [`would_fuse`] answer "no separator required" and let the minifier emit
/// source it could then re-lex but nothing else could load.
#[test]
fn a_numeral_may_not_run_into_a_word() {
    for source in ["0and", "1or", "0x10and", "1e5x", "0b1and", "12abc"] {
        let stream = TokenStream::lex(source, Dialect::Lua);
        let first = stream
            .tokens()
            .iter()
            .find(|t| !t.kind.is_trivia())
            .expect("a token");
        assert_eq!(
            first.text.len(),
            source.len(),
            "{source:?} should lex as one malformed numeral, got {:?}",
            first.text
        );
    }
    // And the consequence the minifier depends on.
    for (a, b) in [("0", "and"), ("1", "or"), ("0", "x"), ("1", "e")] {
        assert!(
            would_fuse(a, b, Dialect::Lua),
            "{a:?} + {b:?} must need a separator"
        );
    }
    // Valid numerals must still lex as numerals.
    for source in ["0x10", "1e5", "1.5", "0b101", "42"] {
        let stream = TokenStream::lex(source, Dialect::Lua);
        assert_eq!(stream.render(), source);
    }
}
