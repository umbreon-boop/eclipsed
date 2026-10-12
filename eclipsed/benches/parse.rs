use criterion::{Criterion, black_box, criterion_group, criterion_main};
use eclipsed::{Dialect, api};
use std::sync::LazyLock;

static LUA_SOURCE: LazyLock<String> =
    LazyLock::new(|| "local value = { x = 1, y = 2, label = 'sample' }\n".repeat(256));
const LUAU_TYPES_SOURCE: &str =
    include_str!("../tests/corpus/roblox_cases/pass/type_instantiations/source.lua");
const LUAU_INTERPOLATION_SOURCE: &str =
    include_str!("../tests/corpus/roblox_cases/pass/string_interpolation_conformance/source.lua");

fn parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");
    for (name, source, dialect) in [
        ("luau/type_instantiations", LUAU_TYPES_SOURCE, Dialect::Luau),
        (
            "luau/string_interpolation",
            LUAU_INTERPOLATION_SOURCE,
            Dialect::Luau,
        ),
    ] {
        group.bench_function(name, |b| {
            b.iter(|| {
                api::with_chunk(black_box(source), dialect, |_| ()).expect("benchmark parses")
            });
        });
    }
    group.bench_function("lua54/256_local_tables", |b| {
        b.iter(|| {
            api::with_chunk(black_box(&**LUA_SOURCE), Dialect::Lua, |_| ())
                .expect("benchmark parses")
        });
    });
    group.finish();
}

criterion_group!(benches, parse);
criterion_main!(benches);
