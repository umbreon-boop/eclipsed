//! A documentation extractor — the moonwave-shaped tool.
//!
//! Usage:
//!   cargo run --release --example docs --features cst,luau,luajit,cfxlua -- [--dialect luau] [--json] <path>
//!
//! The difference from moonwave is the source of truth. Moonwave reads only
//! comments, so it cannot tell when a `@param` names an argument that no
//! longer exists. This reads the **AST** for signatures and uses comments only
//! as annotation, which means it can cross-check the two and report drift.
//!
//! It also does not decide what a doc comment looks like. `---` is used here
//! because that is the common convention, but each declaration's `doc` holds
//! every comment above it and lets the caller choose.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use eclipsed::api::{self, Chunk};
use eclipsed::declarations::{DeclarationKind, NameJoin};
use eclipsed::dialect::Dialect;

/// One documented (or undocumented) declaration.
struct Item {
    kind: &'static str,
    name: String,
    params: Vec<String>,
    doc: Vec<String>,
    /// `@param` names in the doc block that no parameter matches.
    drift: Vec<String>,
}

fn doc_lines(chunk: &Chunk<'_>, doc: &[eclipsed::lexer::Span]) -> Vec<String> {
    doc.iter()
        .map(|span| chunk.text(*span))
        // Two conventions in the wild: `---` line comments (LDoc-ish) and
        // `--[[ ]]` / `--[=[ ]=]` blocks (what moonwave uses). Accept both.
        // The library imposes neither — this is the example choosing.
        .flat_map(|text| {
            if let Some(rest) = text.strip_prefix("---") {
                vec![rest.trim().to_string()]
            } else if text.starts_with("--[") {
                // Strip the long-bracket delimiters, keep the interior lines.
                let inner = text
                    .trim_start_matches('-')
                    .trim_start_matches('[')
                    .trim_start_matches('=')
                    .trim_start_matches('[');
                let inner = match inner.rfind("]") {
                    Some(end) => &inner[..end],
                    None => inner,
                };
                inner
                    .lines()
                    .map(|line| {
                        line.trim()
                            .trim_end_matches(']')
                            .trim_end_matches('=')
                            .to_string()
                    })
                    .collect()
            } else {
                Vec::new()
            }
        })
        .filter(|line| !line.is_empty())
        // Drop residue of the closing long bracket (`]]`, `]=]`, ...).
        .filter(|line| !line.chars().all(|c| c == ']' || c == '=' || c == '['))
        .collect()
}

/// `@param name ...` / `@arg name ...` mentioned in a doc block.
fn documented_params(doc: &[String]) -> Vec<String> {
    doc.iter()
        .filter_map(|line| {
            let rest = line
                .strip_prefix("@param ")
                .or_else(|| line.strip_prefix("@arg "))?;
            rest.split_whitespace().next().map(str::to_string)
        })
        .collect()
}

fn collect(chunk: &Chunk<'_>, out: &mut Vec<Item>) {
    for decl in chunk.declarations() {
        let kind = match (decl.kind, decl.local) {
            (DeclarationKind::Function, true) => "local function",
            (DeclarationKind::Function, false) => "function",
            (DeclarationKind::Field, _) => "field",
            (DeclarationKind::Type, _) => "type",
            (DeclarationKind::TypeFunction, _) => "type function",
        };
        let mut name = String::new();
        for (index, (join, span)) in decl.name.iter().enumerate() {
            if index > 0 {
                name.push(if *join == NameJoin::Method { ':' } else { '.' });
            }
            name.push_str(chunk.text(*span));
        }
        let mut params: Vec<String> = decl
            .parameters
            .iter()
            .map(|span| chunk.text(*span).to_string())
            .collect();
        if decl.vararg {
            params.push("...".to_string());
        }
        let doc = doc_lines(chunk, &decl.doc);
        if doc.is_empty() && kind == "field" {
            continue;
        }
        let drift = documented_params(&doc)
            .into_iter()
            .filter(|m| !params.contains(m))
            .collect();
        out.push(Item {
            kind,
            name,
            params,
            doc,
            drift,
        });
    }
}

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
    let mut json = false;
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
            "--json" => json = true,
            other => path = Some(PathBuf::from(other)),
        }
    }
    let root = path.expect("usage: docs [--dialect <name>] [--json] <path>");

    let (mut documented, mut undocumented, mut drifted) = (0usize, 0usize, 0usize);

    for file in lua_files(&root) {
        let Ok(source) = fs::read_to_string(&file) else {
            continue;
        };
        let items = match api::with_chunk(&source, dialect, |chunk| {
            let mut items = Vec::new();
            collect(chunk, &mut items);
            items
        }) {
            Ok(items) => items,
            Err(error) => {
                eprintln!("PARSE ERROR {}: {error}", file.display());
                continue;
            }
        };
        if items.is_empty() {
            continue;
        }

        if !json {
            println!("\n## {}", file.display());
        }
        for item in items {
            if item.doc.is_empty() {
                undocumented += 1;
            } else {
                documented += 1;
            }
            if !item.drift.is_empty() {
                drifted += 1;
            }

            if json {
                println!(
                    "{{\"file\":{:?},\"kind\":{:?},\"name\":{:?},\"params\":{:?},\"doc\":{:?},\"drift\":{:?}}}",
                    file.display().to_string(),
                    item.kind,
                    item.name,
                    item.params,
                    item.doc,
                    item.drift
                );
            } else {
                let signature = if item.kind == "type" || item.kind == "field" {
                    item.name.clone()
                } else {
                    format!("{}({})", item.name, item.params.join(", "))
                };
                println!("\n### {} `{}`", item.kind, signature);
                if item.doc.is_empty() {
                    println!("_undocumented_");
                } else {
                    for line in &item.doc {
                        println!("{line}");
                    }
                }
                for name in &item.drift {
                    // A hint, never a failure: `@param` may legitimately
                    // describe a field of a table argument, a vararg, or a
                    // parameter of a closure the function returns. Surfacing
                    // it is useful; enforcing it would be wrong.
                    println!("> warning: `@param {name}` matches no parameter of this signature");
                }
            }
        }
    }

    eprintln!(
        "\n{documented} documented, {undocumented} undocumented, {drifted} with @param drift"
    );
    ExitCode::SUCCESS
}
