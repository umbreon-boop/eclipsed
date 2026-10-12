//! A minifier, in about thirty lines — the darklua-shaped tool.
//!
//! Minifying is one pass over the token stream: drop the trivia, keeping a
//! separator only where two tokens would otherwise fuse. There is no printer
//! to configure, because there is no layout decision left to make.
//!
//! Usage:
//!   cargo run --release --example minify --features cst,luau,luajit,cfxlua -- [--dialect luau] [--check] <path>
//!
//! `--check` verifies the output still parses and reports the size saved,
//! which is what you want wired into CI.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use eclipsed::api;
use eclipsed::dialect::Dialect;
use eclipsed::stream::{TokenStream, strip_trivia};

fn lua_files(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "lua" || e == "luau") {
                out.push(path);
            }
        }
    }
    if root.is_file() {
        return vec![root.to_path_buf()];
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out.sort();
    out
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let mut dialect = Dialect::Luau;
    let mut check = false;
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
            "--check" => check = true,
            other => path = Some(PathBuf::from(other)),
        }
    }
    let root = path.expect("usage: minify [--dialect <name>] [--check] <path>");

    let (mut before, mut after, mut failed) = (0usize, 0usize, 0usize);
    for file in lua_files(&root) {
        let Ok(source) = fs::read_to_string(&file) else {
            continue;
        };
        let mut stream = TokenStream::lex(&source, dialect);
        strip_trivia(&mut stream, dialect);
        let small = stream.render();

        // Verifying the output reparses is the difference between a minifier
        // you can put in CI and one you cannot. Lexing cannot fail, so this is
        // also where a file that was never valid Lua gets caught.
        if check && let Err(error) = api::with_chunk(&small, dialect, |_| ()) {
            eprintln!("BROKEN OUTPUT  {}: {error}", file.display());
            failed += 1;
            continue;
        }
        before += source.len();
        after += small.len();
        println!(
            "{:>7} -> {:>7}  ({:>5.1}%)  {}",
            source.len(),
            small.len(),
            100.0 * small.len() as f64 / source.len().max(1) as f64,
            file.display()
        );
    }

    println!(
        "\ntotal {before} -> {after} bytes ({:.1}% of original), {failed} failure(s)",
        100.0 * after as f64 / before.max(1) as f64
    );
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
