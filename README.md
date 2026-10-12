<div align="center">
	<h1>
		Eclipsed<br>
		<a href="https://crates.io/crates/eclipsed"><img src="https://img.shields.io/crates/v/eclipsed.svg"></a>
    <a href="https://docs.rs/eclipsed"><img src="https://img.shields.io/docsrs/eclipsed"></a>
	</h1>
</div>

Eclipsed is a lossless Lua 5.1 / 5.2 / 5.3 / 5.4 / [Luau](https://luau-lang.org/) / [CfxLua](https://docs.fivem.net/docs/scripting-manual/runtimes/lua/) parser designed for editing the syntax tree in batched passes.

## Lossless

Eclipsed preserves comments, whitespace, and style choices. Parsing Lua into a syntax tree and printing it back reproduces the original code exactly.

## Why

Eclipsed has C bindings, so it can be used from outside Rust. Because the tree itself is built for editing, simple transformations can be done directly on it, without converting to an intermediate representation and back.

## Inspiration

Eclipsed is heavily inspired by [Kampfkarren's full-moon](https://github.com/Kampfkarren/full-moon).