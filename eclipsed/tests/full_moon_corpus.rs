//! Runs eclipsed's parser-conformance corpus (`tests/corpus/`).
//!
//! The `source.lua` cases are vendored from full-moon (origin of the suite) so
//! eclipsed is self-contained and does not depend on the `full-moon/` tree.
//! Cases are organised as `<corpus>/{pass,fail}/<case>/source.lua`, one corpus
//! per Lua version. eclipsed's `Dialect::Lua` is vanilla Lua 5.4 — a grammar
//! superset of 5.1/5.2/5.3 — so every 5.x pass corpus must parse under it. Fail
//! cases are expected to be rejected, except the handful full-moon ran as Lua
//! 5.1 but which are legitimately valid 5.2+/5.4 syntax; those are listed per
//! corpus in `VALID_IN_LUA54` with a justification.
//!
//! To refresh the corpus from an updated full-moon: copy the `source.lua` files
//! under its `tests/{cases,lua5*_cases,luajit_cases,cfxlua_cases,roblox_cases}`
//! into `tests/corpus/`, preserving the directory structure.

use std::fs;
use std::path::PathBuf;

use bumpalo::Bump;
use eclipsed::dialect::Dialect;
use eclipsed::interner::Interner;
use eclipsed::lexer::Lexer;
use eclipsed::parser::Parser;

/// Fail cases full-moon rejects as Lua 5.1 that are valid Lua 5.4, which is
/// what `Dialect::Lua` parses. Each entry is `(case name, why it's valid)`.
const VALID_IN_LUA54: &[(&str, &str)] = &[(
    "stmt-after-break-1",
    "since Lua 5.2, `break` may appear mid-block; only 5.1 requires it last",
)];

/// Fail cases eclipsed accepts by design: `Dialect::Lua` is a permissive
/// superset that keeps 5.1 compatibility where it does not conflict with 5.4.
const PERMISSIVE_LUA: &[(&str, &str)] = &[(
    "goto-as-identifier",
    "eclipsed keeps `goto` usable in field position for 5.1/LuaJIT sources \
     (cases/pass/goto-as-identifier exercises the same source as a pass case)",
)];

fn corpus_root() -> PathBuf {
    // Self-contained: the corpus lives inside this crate (vendored from full-moon).
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus")
}

fn try_parse(source: &str, dialect: Dialect) -> Result<(), String> {
    let arena = Bump::new();
    let tokens = Lexer::new(source, &arena, dialect).lex();
    let mut interner = Interner::new();
    let mut parser = Parser::new(tokens, source, &arena, &mut interner, dialect);
    parser
        .parse()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Every `<case>/source.lua` under `folder`, sorted for stable output.
fn cases_in(folder: &str) -> Vec<(String, String)> {
    let root = corpus_root().join(folder);
    let mut cases: Vec<(String, String)> = fs::read_dir(&root)
        .unwrap_or_else(|error| panic!("couldn't read corpus folder {root:?}: {error}"))
        .map(|entry| entry.expect("couldn't read corpus entry").path())
        .filter(|path| path.is_dir())
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let source = fs::read_to_string(path.join("source.lua"))
                .unwrap_or_else(|error| panic!("couldn't read {path:?}/source.lua: {error}"));
            (name, source)
        })
        .collect();
    cases.sort();
    cases
}

fn run_pass_corpus(folder: &str, dialect: Dialect) {
    let mut failures = Vec::new();
    for (name, source) in cases_in(folder) {
        if let Err(error) = try_parse(&source, dialect) {
            failures.push(format!("  {folder}/{name}: {error}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} pass case(s) failed to parse:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn run_fail_corpus(folder: &str, dialect: Dialect, exceptions: &[(&str, &str)]) {
    let mut failures = Vec::new();
    for (name, source) in cases_in(folder) {
        let exempt = exceptions.iter().any(|(case, _)| *case == name);
        if try_parse(&source, dialect).is_ok() && !exempt {
            failures.push(format!("  {folder}/{name}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} fail case(s) parsed without error:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ---- Lua 5.1 base corpus (parsed as 5.4, a grammar superset) ----

#[test]
fn lua51_pass_cases() {
    run_pass_corpus("cases/pass", Dialect::Lua);
}

#[test]
fn lua51_parser_fail_cases() {
    run_fail_corpus("cases/fail/parser", Dialect::Lua, VALID_IN_LUA54);
}

#[test]
fn lua51_tokenizer_fail_cases() {
    run_fail_corpus("cases/fail/tokenizer", Dialect::Lua, VALID_IN_LUA54);
}

// ---- Lua 5.2 / 5.3 / 5.4 corpora ----

#[test]
fn lua52_pass_cases() {
    run_pass_corpus("lua52_cases/pass", Dialect::Lua);
}

#[test]
fn lua52_parser_fail_cases() {
    run_fail_corpus("lua52_cases/fail/parser", Dialect::Lua, PERMISSIVE_LUA);
}

#[test]
fn lua53_pass_cases() {
    run_pass_corpus("lua53_cases/pass", Dialect::Lua);
}

#[test]
fn lua53_parser_fail_cases() {
    run_fail_corpus("lua53_cases/fail/parser", Dialect::Lua, VALID_IN_LUA54);
}

#[test]
fn lua54_pass_cases() {
    run_pass_corpus("lua54_cases/pass", Dialect::Lua);
}

#[test]
fn lua54_parser_fail_cases() {
    run_fail_corpus("lua54_cases/fail/parser", Dialect::Lua, &[]);
}

// ---- LuaJIT ----

#[cfg(feature = "luajit")]
#[test]
fn luajit_pass_cases() {
    run_pass_corpus("luajit_cases/pass", Dialect::LuaJit);
}

// ---- CfxLua (FiveM) ----

#[cfg(feature = "cfxlua")]
#[test]
fn cfxlua_pass_cases() {
    run_pass_corpus("cfxlua_cases/pass", Dialect::CfxLua);
}

#[cfg(feature = "cfxlua")]
#[test]
fn cfxlua_parser_fail_cases() {
    run_fail_corpus("cfxlua_cases/fail/parser", Dialect::CfxLua, &[]);
}

#[cfg(feature = "cfxlua")]
#[test]
fn cfxlua_tokenizer_fail_cases() {
    run_fail_corpus("cfxlua_cases/fail/tokenizer", Dialect::CfxLua, &[]);
}

// ---- Luau (full-moon's "roblox" corpus) ----

#[cfg(feature = "luau")]
#[test]
fn luau_pass_cases() {
    run_pass_corpus("roblox_cases/pass", Dialect::Luau);
}

#[cfg(feature = "luau")]
#[test]
fn luau_parser_fail_cases() {
    run_fail_corpus("roblox_cases/fail/parser", Dialect::Luau, &[]);
}

#[cfg(feature = "luau")]
#[test]
fn luau_tokenizer_fail_cases() {
    run_fail_corpus("roblox_cases/fail/tokenizer", Dialect::Luau, &[]);
}
