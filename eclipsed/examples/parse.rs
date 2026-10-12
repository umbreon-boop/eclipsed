//! Lex + parse a file and report the outcome.
//!
//! Usage: `cargo run --example parse [--dialect <lua|luau|luajit|cfxlua>] <file.lua>`

use std::env;
use std::fs;
use std::process::ExitCode;

use bumpalo::Bump;
use eclipsed::dialect::Dialect;
use eclipsed::interner::Interner;
use eclipsed::lexer::Lexer;
use eclipsed::parser::Parser;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let mut dialect = Dialect::Lua;
    let mut path = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dialect" | "-d" => {
                dialect = args
                    .next()
                    .expect("--dialect needs a value")
                    .parse()
                    .unwrap_or_else(|error| panic!("{error}"));
            }
            other => path = Some(other.to_string()),
        }
    }

    let path = path.expect("usage: parse [--dialect <name>] <file.lua>");
    let source = fs::read_to_string(&path).expect("couldn't read source file");

    let arena = Bump::new();
    let tokens = Lexer::new(&source, &arena, dialect).lex();
    println!("{}: {} tokens", path, tokens.len());

    let mut interner = Interner::new();
    let mut parser = Parser::new(tokens, &source, &arena, &mut interner, dialect);
    match parser.parse() {
        Ok(statements) => {
            println!("{path}: parsed {} top-level statement(s)", statements.len());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{path}: {error}");
            ExitCode::FAILURE
        }
    }
}
