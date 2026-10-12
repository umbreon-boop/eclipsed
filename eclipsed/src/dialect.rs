//! The Lua dialects the lexer and parser understand.

use std::fmt;
use std::str::FromStr;

/// The Lua dialect to lex and parse as.
///
/// [`Dialect::Lua`] (Lua 5.4) is always available; the others exist only when
/// their cargo feature is enabled. Dialects are permissive supersets of a
/// shared core, so each may accept some syntax it does not strictly own.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Dialect {
    /// Lua 5.4.
    #[default]
    Lua,
    /// Luau: Lua 5.1 plus types, compound assignment, `continue`, `if`
    /// expressions and string interpolation.
    #[cfg(feature = "luau")]
    Luau,
    /// LuaJIT: Lua 5.1 plus cdata literal suffixes.
    #[cfg(feature = "luajit")]
    LuaJit,
    /// CfxLua (FiveM): Lua 5.4 plus compound assignment, `?.`, set
    /// constructors, `in` unpacking, C comments and jenkins hash strings.
    #[cfg(feature = "cfxlua")]
    CfxLua,
}

impl Dialect {
    /// Every dialect compiled into this build.
    pub const ALL: &'static [Dialect] = &[
        Dialect::Lua,
        #[cfg(feature = "luau")]
        Dialect::Luau,
        #[cfg(feature = "luajit")]
        Dialect::LuaJit,
        #[cfg(feature = "cfxlua")]
        Dialect::CfxLua,
    ];

    /// The lowercase name accepted by [`FromStr`], e.g. `"luajit"`.
    pub const fn name(self) -> &'static str {
        match self {
            Dialect::Lua => "lua",
            #[cfg(feature = "luau")]
            Dialect::Luau => "luau",
            #[cfg(feature = "luajit")]
            Dialect::LuaJit => "luajit",
            #[cfg(feature = "cfxlua")]
            Dialect::CfxLua => "cfxlua",
        }
    }

    /// The Luau type grammar: annotations, generics, unions and the rest.
    #[inline]
    pub fn has_types(self) -> bool {
        self.is_luau()
    }

    /// `+=`, `..=` and the other arithmetic compound assignments.
    #[inline]
    pub fn has_compound_assignment(self) -> bool {
        self.is_luau() || self.is_cfxlua()
    }

    /// `&=`, `|=`, `<<=` and `>>=`.
    #[inline]
    pub fn has_bitwise_compound_assignment(self) -> bool {
        self.is_cfxlua()
    }

    /// `/* ... */` comments.
    #[inline]
    pub fn has_c_style_comments(self) -> bool {
        self.is_cfxlua()
    }

    /// Backtick compile-time jenkins hash strings.
    #[inline]
    pub fn has_jenkins_hash_strings(self) -> bool {
        self.is_cfxlua()
    }

    /// `?.` safe navigation.
    #[inline]
    pub fn has_safe_navigation(self) -> bool {
        self.is_cfxlua()
    }

    /// `{ .name }` set constructor fields.
    #[inline]
    pub fn has_set_constructor(self) -> bool {
        self.is_cfxlua()
    }

    /// `local a, b in t` unpacking.
    #[inline]
    pub fn has_in_unpacking(self) -> bool {
        self.is_cfxlua()
    }

    /// Whether a `(` on a later line than a nested callee starts a new
    /// statement instead of call arguments.
    ///
    /// Real Luau rejects `f` newline `(x)` outright; accepting it as two
    /// statements is deliberately more permissive, because obfuscated Luau
    /// relies on it.
    #[inline]
    pub fn breaks_ambiguous_call(self) -> bool {
        self.is_luau()
    }

    /// The `continue` statement.
    #[inline]
    pub fn has_continue(self) -> bool {
        self.is_luau()
    }

    /// Generalized iteration: `for k, v in t do` over a table directly.
    #[inline]
    pub fn has_generalized_iteration(self) -> bool {
        self.is_luau()
    }

    /// `if c then a else b` expressions.
    #[inline]
    pub fn has_if_expression(self) -> bool {
        self.is_luau()
    }

    /// Backtick string interpolation.
    #[inline]
    pub fn has_string_interpolation(self) -> bool {
        self.is_luau()
    }

    /// `@attribute` markers before functions.
    #[inline]
    pub fn has_attributes(self) -> bool {
        self.is_luau()
    }

    /// The `::` cast operator and explicit instantiation (`f<<T>>()`).
    #[inline]
    pub fn has_type_cast(self) -> bool {
        self.is_luau()
    }

    /// `const` declarations.
    #[inline]
    pub fn has_const_declarations(self) -> bool {
        self.is_luau()
    }

    /// `type`, `export`, `continue` and `typeof` are contextual keywords.
    #[inline]
    pub fn keywords_are_contextual(self) -> bool {
        self.is_luau()
    }

    /// Intersection types (`A & B`).
    #[inline]
    pub fn has_intersection_types(self) -> bool {
        self.is_luau()
    }

    /// `typeof(expr)` types.
    #[inline]
    pub fn has_typeof_type(self) -> bool {
        self.is_luau()
    }

    /// Singleton string and boolean types.
    #[inline]
    pub fn has_singleton_types(self) -> bool {
        self.is_luau()
    }

    /// `LL`, `ULL` and `i` numeric literal suffixes.
    #[inline]
    pub fn has_cdata_suffixes(self) -> bool {
        self.is_luajit()
    }

    /// `_` separators in numeric literals (`1_000`).
    #[inline]
    pub fn has_digit_separators(self) -> bool {
        self.is_luau()
    }

    /// Bytes `0x80..=0xFF` are identifier characters, as in LuaJIT.
    #[inline]
    pub fn has_extended_identifiers(self) -> bool {
        self.is_luajit()
    }

    #[inline]
    fn is_luau(self) -> bool {
        #[cfg(feature = "luau")]
        return matches!(self, Dialect::Luau);
        #[cfg(not(feature = "luau"))]
        false
    }

    #[inline]
    fn is_luajit(self) -> bool {
        #[cfg(feature = "luajit")]
        return matches!(self, Dialect::LuaJit);
        #[cfg(not(feature = "luajit"))]
        false
    }

    #[inline]
    fn is_cfxlua(self) -> bool {
        #[cfg(feature = "cfxlua")]
        return matches!(self, Dialect::CfxLua);
        #[cfg(not(feature = "cfxlua"))]
        false
    }
}

impl fmt::Display for Dialect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Error returned when parsing an unknown or disabled [`Dialect`] name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown or disabled dialect {0:?}")]
pub struct UnknownDialect(pub String);

impl FromStr for Dialect {
    type Err = UnknownDialect;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Dialect::ALL
            .iter()
            .copied()
            .find(|dialect| dialect.name().eq_ignore_ascii_case(name))
            .ok_or_else(|| UnknownDialect(name.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for &dialect in Dialect::ALL {
            assert_eq!(dialect.name().parse::<Dialect>(), Ok(dialect));
        }
        assert!("perl".parse::<Dialect>().is_err());
    }
}
