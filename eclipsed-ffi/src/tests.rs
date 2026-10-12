use std::ffi::CStr;
use std::ptr;

use crate::*;

fn zeroed_error() -> EclError {
    EclError {
        status: EclStatus::Ok as u32,
        start: 0,
        end: 0,
        message: ptr::null_mut(),
    }
}

fn message(err: &EclError) -> String {
    // SAFETY: set by the library to a NUL-terminated string.
    unsafe { CStr::from_ptr(err.message) }
        .to_string_lossy()
        .into_owned()
}

fn check(source: &[u8], dialect: EclDialect) -> (EclStatus, EclError) {
    let mut err = zeroed_error();
    // SAFETY: `source` is a live slice and `err` a valid out-pointer.
    let status = unsafe {
        ecl_check(
            source.as_ptr().cast(),
            source.len(),
            dialect as u32,
            &mut err,
        )
    };
    (status, err)
}

fn parse(source: &str, dialect: EclDialect) -> *mut EclChunk {
    let mut chunk = ptr::null_mut();
    // SAFETY: valid pointers throughout.
    let status = unsafe {
        ecl_parse(
            source.as_ptr().cast(),
            source.len(),
            dialect as u32,
            &mut chunk,
            ptr::null_mut(),
        )
    };
    assert_eq!(status, EclStatus::Ok);
    chunk
}

fn slice<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
    if len == 0 {
        &[]
    } else {
        // SAFETY: the library returned `len` elements at `ptr`.
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }
}

#[test]
fn check_accepts_valid_source() {
    let (status, err) = check(b"local x = 1", EclDialect::Lua);
    assert_eq!(status, EclStatus::Ok);
    assert!(err.message.is_null());
}

#[test]
fn parse_error_reports_its_span() {
    let (status, mut err) = check(b"local = 1", EclDialect::Lua);
    assert_eq!(status, EclStatus::Parse);
    assert_eq!((err.start, err.end), (6, 7));
    assert!(message(&err).contains("variable name"));
    // SAFETY: `err` holds a message owned by the library.
    unsafe { ecl_error_free(&mut err) };
    assert!(err.message.is_null());
    // SAFETY: freeing twice is allowed.
    unsafe { ecl_error_free(&mut err) };
}

#[test]
fn reused_error_replaces_its_message() {
    let mut err = zeroed_error();
    for source in ["local = 1", "return )"] {
        // SAFETY: valid pointers.
        unsafe { ecl_check(source.as_ptr().cast(), source.len(), 0, &mut err) };
    }
    assert!(message(&err).contains("')'"), "{}", message(&err));
    // SAFETY: owned by the library.
    unsafe { ecl_error_free(&mut err) };
}

#[test]
fn invalid_utf8_reports_the_offset() {
    let (status, mut err) = check(b"x = '\xff'", EclDialect::Lua);
    assert_eq!(status, EclStatus::InvalidUtf8);
    assert_eq!((err.start, err.end), (5, 6));
    // SAFETY: owned by the library.
    unsafe { ecl_error_free(&mut err) };
}

#[test]
fn unknown_dialect_is_rejected() {
    let mut err = zeroed_error();
    // SAFETY: valid pointers.
    let status = unsafe { ecl_check(c"".as_ptr(), 0, 99, &mut err) };
    assert_eq!(status, EclStatus::InvalidDialect);
    // SAFETY: owned by the library.
    unsafe { ecl_error_free(&mut err) };
}

#[test]
fn null_source_is_empty_only_with_zero_length() {
    // SAFETY: null is permitted and `err` may be null.
    unsafe {
        assert_eq!(ecl_check(ptr::null(), 0, 0, ptr::null_mut()), EclStatus::Ok);
        assert_eq!(
            ecl_check(ptr::null(), 3, 0, ptr::null_mut()),
            EclStatus::NullArgument
        );
    }
}

#[test]
fn lex_tiles_the_source() {
    let source = "-- hi\nlocal x = 1 --[[ c ]]\n";
    let mut array = EclTokenArray {
        ptr: ptr::null_mut(),
        len: 0,
    };
    // SAFETY: valid pointers.
    let status = unsafe {
        ecl_lex(
            source.as_ptr().cast(),
            source.len(),
            0,
            &mut array,
            ptr::null_mut(),
        )
    };
    assert_eq!(status, EclStatus::Ok);
    let tokens = slice(array.ptr, array.len);
    let rebuilt: String = tokens
        .iter()
        .map(|t| &source[t.start as usize..t.end as usize])
        .collect();
    assert_eq!(rebuilt, source);
    assert_eq!(tokens[0].token_type, EclTrivia::LineComment as u32);
    let local = tokens
        .iter()
        .find(|t| t.token_type < EclTrivia::Whitespace as u32)
        .unwrap();
    // SAFETY: static strings.
    let names = unsafe {
        [local.token_type, tokens[0].token_type].map(|id| CStr::from_ptr(ecl_token_type_name(id)))
    };
    assert_eq!(names, [c"Local", c"LineComment"]);
    // SAFETY: produced by `ecl_lex`; freeing twice is allowed.
    unsafe {
        ecl_token_array_free(&mut array);
        ecl_token_array_free(&mut array);
    }
}

#[test]
fn null_output_pointer_is_an_error_not_a_leak() {
    // SAFETY: a null `out` is reported, not written.
    let status = unsafe { ecl_lex(c"x".as_ptr(), 1, 0, ptr::null_mut(), ptr::null_mut()) };
    assert_eq!(status, EclStatus::NullArgument);
}

#[test]
fn minify_keeps_load_bearing_spaces() {
    let source = "local  x = 1 -- c\nreturn  x";
    let mut out = EclString {
        ptr: ptr::null_mut(),
        len: 0,
    };
    // SAFETY: valid pointers.
    let status = unsafe {
        ecl_minify(
            source.as_ptr().cast(),
            source.len(),
            0,
            &mut out,
            ptr::null_mut(),
        )
    };
    assert_eq!(status, EclStatus::Ok);
    let text = slice(out.ptr.cast::<u8>(), out.len + 1);
    assert_eq!(text, b"local x=1 return x\0");
    // SAFETY: produced by `ecl_minify`.
    unsafe { ecl_string_free(&mut out) };
}

#[test]
fn needs_space() {
    let ask = |a: &str, b: &str| {
        // SAFETY: live slices.
        unsafe { ecl_needs_space(a.as_ptr().cast(), a.len(), b.as_ptr().cast(), b.len(), 0) }
    };
    assert_eq!(ask("local", "x"), 1);
    assert_eq!(ask("-", "-x"), 1);
    assert_eq!(ask("x", "="), 0);
    // SAFETY: null with a length is rejected.
    let invalid = unsafe { ecl_needs_space(ptr::null(), 1, c"x".as_ptr(), 1, 0) };
    assert_eq!(invalid, -1);
}

#[test]
fn dialect_names_round_trip() {
    for id in 0..4 {
        let name = ecl_dialect_name(id);
        // SAFETY: a static NUL-terminated string.
        assert_eq!(unsafe { ecl_dialect_from_name(name) }, id as i32);
    }
    assert!(ecl_dialect_name(4).is_null());
    // SAFETY: a valid C string, and null.
    unsafe {
        assert_eq!(ecl_dialect_from_name(c"perl".as_ptr()), -1);
        assert_eq!(ecl_dialect_from_name(ptr::null()), -1);
    }
}

#[test]
fn token_type_names_cover_every_id() {
    let count = ecl_token_type_count();
    assert!((0..count).all(|id| !ecl_token_type_name(id).is_null()));
    assert!(ecl_token_type_name(count).is_null());
}

#[test]
fn version_strings() {
    assert_eq!(ecl_abi_version(), ECL_ABI_VERSION);
    // SAFETY: a static string.
    let version = unsafe { CStr::from_ptr(ecl_version()) };
    assert_eq!(version.to_str().unwrap(), env!("CARGO_PKG_VERSION"));
}

#[test]
fn chunk_type_spans() {
    let source = "local x: number = 1\ntype T = string";
    let chunk = parse(source, EclDialect::Luau);
    let mut len = 0;
    // SAFETY: `chunk` is live.
    let spans = slice(unsafe { ecl_chunk_type_spans(chunk, &mut len) }, len);
    let texts: Vec<_> = spans
        .iter()
        .map(|s| (s.kind, &source[s.start as usize..s.end as usize]))
        .collect();
    assert_eq!(
        texts,
        [
            (EclTypeSpanKind::Annotation, "number"),
            (EclTypeSpanKind::AliasStatement, "type T = string")
        ]
    );
    let mut count = 0;
    // SAFETY: `chunk` is live.
    assert!(!unsafe { ecl_chunk_statements(chunk, &mut count) }.is_null());
    assert_eq!(count, 2);
    // SAFETY: produced by `ecl_parse`.
    unsafe { ecl_chunk_free(chunk) };
}

/// Renders declaration rows as `kind name(params) [docs]` lines.
fn declarations(source: &str, dialect: EclDialect) -> Vec<String> {
    let chunk = parse(source, dialect);
    let mut len = 0;
    // SAFETY: `chunk` is live.
    let rows = slice(unsafe { ecl_chunk_declarations(chunk, &mut len) }, len);
    let text = |row: &EclDecl| &source[row.start as usize..row.end as usize];
    let mut out: Vec<String> = Vec::new();
    for row in rows {
        match row.record {
            EclRecord::Decl => out.push(format!("{} ", row.kind)),
            EclRecord::Name => {
                let line = out.last_mut().unwrap();
                if !line.ends_with(' ') {
                    line.push(if row.kind == EclNameJoin::Method as u32 {
                        ':'
                    } else {
                        '.'
                    });
                }
                line.push_str(text(row));
            }
            EclRecord::Param => out.last_mut().unwrap().push_str(&format!(" {}", text(row))),
            EclRecord::Vararg => out.last_mut().unwrap().push_str(" ..."),
            EclRecord::Doc => out
                .last_mut()
                .unwrap()
                .push_str(&format!(" [{}]", text(row))),
        }
    }
    // SAFETY: produced by `ecl_parse`.
    unsafe { ecl_chunk_free(chunk) };
    out
}

#[test]
fn declarations_cover_the_common_forms() {
    let source = "\
--- adds
function M.add(a, b) end
local function helper(...) end
local f = function(x) end
M.value, M.other = 1, function() end
if cond then
elseif other then
  function M.late() end
end
function M:method(y) end
return { exported = function(z) end }
";
    let function = EclDeclKind::Function as u32;
    let field = EclDeclKind::Field as u32;
    assert_eq!(
        declarations(source, EclDialect::Lua),
        [
            format!("{function} M.add a b [--- adds]"),
            format!("{function} helper ..."),
            format!("{function} f x"),
            format!("{field} M.value"),
            format!("{function} M.other"),
            format!("{function} M.late"),
            format!("{function} M:method y"),
            format!("{function} exported z"),
        ]
    );
}

#[test]
fn declarations_include_luau_types() {
    let rows = declarations("type T = number\ntype function g(t) end", EclDialect::Luau);
    assert_eq!(
        rows,
        [
            format!("{} T", EclDeclKind::Type as u32),
            format!("{} g t", EclDeclKind::TypeFunction as u32)
        ]
    );
}

#[test]
fn chunk_accessors_accept_null() {
    let mut len = 7;
    // SAFETY: null chunks are permitted.
    unsafe {
        assert!(ecl_chunk_tokens(ptr::null(), &mut len).is_null());
        assert_eq!(len, 0);
        len = 7;
        assert!(ecl_chunk_statements(ptr::null(), &mut len).is_null());
        assert_eq!(len, 0);
        ecl_chunk_free(ptr::null_mut());
    }
}

#[test]
fn deep_nesting_fails_cleanly() {
    let source = format!("return {}1{}", "(".repeat(100_000), ")".repeat(100_000));
    let (status, mut err) = check(source.as_bytes(), EclDialect::Lua);
    assert_eq!(status, EclStatus::Parse);
    assert!(message(&err).contains("nesting"));
    // SAFETY: owned by the library.
    unsafe { ecl_error_free(&mut err) };
}

#[test]
fn panics_become_errors() {
    let mut err = zeroed_error();
    let status = crate::error::guard(&mut err, || panic!("boom"));
    assert_eq!(status, EclStatus::Panic);
    assert_eq!(message(&err), "internal error: boom");
    // SAFETY: owned by the library.
    unsafe { ecl_error_free(&mut err) };
}

#[test]
fn messages_never_contain_nul() {
    let mut err = zeroed_error();
    crate::error::guard(&mut err, || {
        Err(crate::error::Failure::new(EclStatus::Parse, "a\0b"))
    });
    assert_eq!(message(&err), "ab");
    // SAFETY: owned by the library.
    unsafe { ecl_error_free(&mut err) };
}

fn with_tree(source: &str, dialect: EclDialect, check: impl FnOnce(&[EclStmt])) {
    let chunk = parse(source, dialect);
    let mut len = 0;
    // SAFETY: `chunk` is live until freed below.
    let statements = slice(unsafe { ecl_chunk_statements(chunk, &mut len) }, len);
    check(statements);
    // SAFETY: produced by `ecl_parse`.
    unsafe { ecl_chunk_free(chunk) };
}

fn text(source: &str, range: EclRange) -> &str {
    &source[range.start as usize..range.end as usize]
}

fn bytes(value: EclBytes) -> &'static [u8] {
    // SAFETY: the chunk's arena holds `len + 1` bytes, NUL last.
    let with_nul = unsafe { std::slice::from_raw_parts(value.ptr.cast::<u8>(), value.len + 1) };
    assert_eq!(with_nul[value.len], 0);
    &with_nul[..value.len]
}

#[test]
fn tree_locals_and_decoded_strings() {
    let source = "local x <const>, y = 'a\\n', 0xFFFFFFFFFFFFFFFF";
    with_tree(source, EclDialect::Lua, |statements| {
        assert_eq!(statements.len(), 1);
        let statement = &statements[0];
        assert_eq!(statement.kind, EclStmtKind::Local);
        // SAFETY: `kind` is LOCAL.
        let local = unsafe { statement.data.local };
        let variables = slice(local.variables.ptr, local.variables.len);
        assert_eq!(text(source, variables[0].name), "x");
        assert_eq!(variables[0].attribute, EclAttribute::Const);
        assert_eq!(variables[1].attribute, EclAttribute::None);
        let values = slice(local.values.ptr, local.values.len);
        assert_eq!(values[0].kind, EclExprKind::String);
        // SAFETY: `kind` is STRING, then INTEGER.
        unsafe {
            assert_eq!(bytes(values[0].data.string), b"a\n");
            assert_eq!(values[1].data.integer, -1);
        }
    });
}

#[test]
fn tree_calls_methods_and_functions() {
    let source = "function a.b:c(x, ...) return f\"s\", t:m{1} end";
    with_tree(source, EclDialect::Lua, |statements| {
        let statement = &statements[0];
        assert_eq!(statement.kind, EclStmtKind::Function);
        // SAFETY: `kind` is FUNCTION; the tree is live for the closure.
        unsafe {
            let function = statement.data.function;
            let path: Vec<_> = slice(function.path.ptr, function.path.len)
                .iter()
                .map(|r| text(source, *r))
                .collect();
            assert_eq!(path, ["a", "b"]);
            assert_eq!(text(source, function.method), "c");
            let body = &*function.body;
            assert!(body.vararg);
            assert_eq!(body.parameters.len, 1);
            let returned = &slice(body.body.ptr, body.body.len)[0];
            assert_eq!(returned.kind, EclStmtKind::Return);
            let values = slice(returned.data.return_.ptr, returned.data.return_.len);
            assert_eq!(values[0].kind, EclExprKind::Call);
            assert_eq!(values[0].data.call.arguments.kind, EclArgumentsKind::String);
            assert_eq!(values[1].kind, EclExprKind::MethodCall);
            let method = values[1].data.method_call;
            assert_eq!(text(source, method.method), "m");
            assert_eq!(method.arguments.kind, EclArgumentsKind::Table);
            assert_eq!((*method.receiver).kind, EclExprKind::Name);
        }
    });
}

#[test]
fn tree_luau_types_and_interpolation() {
    let source = "type T<U> = { name: string, [number]: U? }\nlocal s = `a\\t{x}b`";
    with_tree(source, EclDialect::Luau, |statements| {
        // SAFETY: kinds are checked before each union access.
        unsafe {
            assert_eq!(statements[0].kind, EclStmtKind::TypeAlias);
            let alias = statements[0].data.type_alias;
            assert_eq!(text(source, alias.name), "T");
            let value = &*alias.value;
            assert_eq!(value.kind, EclTypeKind::Struct);
            let fields = slice(value.data.struct_.ptr, value.data.struct_.len);
            assert_eq!(text(source, fields[0].name), "name");
            assert!(!fields[1].indexer.is_null());
            assert_eq!((*fields[1].value).kind, EclTypeKind::Optional);

            let local = statements[1].data.local;
            let value = &slice(local.values.ptr, local.values.len)[0];
            assert_eq!(value.kind, EclExprKind::Interpolated);
            let parts = slice(value.data.interpolated.ptr, value.data.interpolated.len);
            assert_eq!(parts.len(), 3);
            assert_eq!(text(source, parts[0].literal), "a\\t");
            assert_eq!(bytes(parts[0].text), b"a\t");
            assert_eq!((*parts[1].expression).kind, EclExprKind::Name);
            assert_eq!(bytes(parts[2].text), b"b");
        }
    });
}

#[test]
fn tree_if_condition_bindings() {
    let source = "if local x: number = value then end\nlocal y = if const z = value then z else 0";
    with_tree(source, EclDialect::Luau, |statements| {
        // SAFETY: each union access follows the checked node kind.
        unsafe {
            assert_eq!(statements[0].kind, EclStmtKind::If);
            let if_ = statements[0].data.if_;
            assert!(if_.binding.present);
            assert!(!if_.binding.is_const);
            assert_eq!(text(source, if_.binding.variable.name), "x");
            assert!(!if_.binding.variable.type_annotation.is_null());

            let local = statements[1].data.local;
            let value = &slice(local.values.ptr, local.values.len)[0];
            assert_eq!(value.kind, EclExprKind::IfElse);
            let expression = value.data.if_else;
            assert!(expression.binding.present);
            assert!(expression.binding.is_const);
            assert_eq!(text(source, expression.binding.variable.name), "z");
        }
    });
}

#[test]
fn tree_control_flow() {
    let source = "if a then elseif b then else end repeat local z until z for i = 1, 2 do end";
    with_tree(source, EclDialect::Lua, |statements| {
        let kinds: Vec<_> = statements.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                EclStmtKind::If,
                EclStmtKind::Repeat,
                EclStmtKind::NumericFor
            ]
        );
        // SAFETY: kinds checked above.
        unsafe {
            let branch = statements[0].data.if_;
            assert!(branch.has_else);
            assert_eq!(branch.elseifs.len, 1);
            assert!(statements[2].data.numeric_for.step.is_null());
        }
    });
}

#[test]
fn deep_trees_build_on_a_small_stack() {
    let depth = 990;
    let source = format!(
        "return {}1{} + {{{}}}",
        "(".repeat(depth),
        ")".repeat(depth),
        "{".repeat(depth / 2) + &"}".repeat(depth / 2)
    );
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            let blocks = format!("{}{}", "do ".repeat(depth / 2), "end ".repeat(depth / 2));
            for (source, dialect) in [
                (&source, EclDialect::Lua),
                (&source, EclDialect::Luau),
                (&blocks, EclDialect::Luau),
            ] {
                let chunk = parse(source, dialect);
                // SAFETY: produced by `ecl_parse`.
                unsafe { ecl_chunk_free(chunk) };
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

/// cbindgen does not always order by-value struct fields before their use,
/// which only a C compiler notices.
#[test]
fn header_compiles_as_c_and_cpp() {
    let header = concat!(env!("CARGO_MANIFEST_DIR"), "/include/eclipsed.h");
    for (compiler, language) in [("cc", "c"), ("c++", "c++")] {
        let Ok(output) = std::process::Command::new(compiler)
            .args(["-x", language, "-fsyntax-only", "-Wall", "-Werror", header])
            .output()
        else {
            eprintln!("skipping: no {compiler}");
            continue;
        };
        assert!(
            output.status.success(),
            "{compiler}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
