//! The abstract syntax tree. Nodes live in the parser's arena and record
//! meaning, not layout.
#![cfg_attr(
    feature = "cst",
    doc = "Trivia lives in [`crate::stream::TokenStream`]."
)]

use crate::interner::Symbol;
use crate::lexer::Span;

pub trait Spanned {
    fn span(&self) -> Span;

    fn span_start(&self) -> u32 {
        self.span().start
    }

    fn span_end(&self) -> u32 {
        self.span().end
    }
}

#[derive(Debug, Clone)]
pub enum Expression<'arena> {
    Nil {
        span: Span,
    },
    True {
        span: Span,
    },
    False {
        span: Span,
    },
    /// Radix-prefixed literals wrap modulo 2^64, as Lua specifies.
    Integer {
        value: i64,
        span: Span,
    },
    Float {
        value: f64,
        span: Span,
    },
    /// `value` is the literal as written, delimiters and escapes included;
    /// [`crate::string::decode_literal`] yields the bytes it denotes.
    String {
        value: Symbol,
        span: Span,
    },
    Vararg {
        span: Span,
    },
    Identifier {
        name: Symbol,
        span: Span,
    },
    Unary {
        operator: UnaryOperator,
        operand: &'arena Expression<'arena>,
        span: Span,
    },
    Binary {
        left: &'arena Expression<'arena>,
        operator: BinaryOperator,
        right: &'arena Expression<'arena>,
        span: Span,
    },
    Table {
        fields: &'arena [TableField<'arena>],
        span: Span,
    },
    Closure {
        body: &'arena FunctionBody<'arena>,
        span: Span,
    },
    Call {
        callee: &'arena Expression<'arena>,
        arguments: CallArguments<'arena>,
        span: Span,
    },
    MethodCall {
        receiver: &'arena Expression<'arena>,
        method: Symbol,
        method_span: Span,
        arguments: CallArguments<'arena>,
        span: Span,
    },
    Field {
        table: &'arena Expression<'arena>,
        field: Symbol,
        field_span: Span,
        /// CfxLua safe navigation (`t?.field`); always `false` elsewhere.
        safe: bool,
        span: Span,
    },
    Index {
        table: &'arena Expression<'arena>,
        key: &'arena Expression<'arena>,
        span: Span,
    },
    Cast {
        expression: &'arena Expression<'arena>,
        target: &'arena TypeAnnotation<'arena>,
        span: Span,
    },
    /// Kept because parentheses truncate a multi-valued expression.
    Paren {
        expression: &'arena Expression<'arena>,
        span: Span,
    },
    /// Luau `if c then a elseif d then b else e` expression.
    #[cfg(feature = "luau")]
    IfElse {
        binding: Option<IfConditionBinding<'arena>>,
        condition: &'arena Expression<'arena>,
        then_branch: &'arena Expression<'arena>,
        elseifs: &'arena [ElseIfExpression<'arena>],
        else_branch: &'arena Expression<'arena>,
        span: Span,
    },
    /// Luau backtick string interpolation, e.g. `` `count: {n}` ``.
    #[cfg(feature = "luau")]
    InterpolatedString {
        parts: &'arena [InterpolationPart<'arena>],
        span: Span,
    },
    /// LuaJIT signed 64-bit integer literal, e.g. `42LL`.
    #[cfg(feature = "luajit")]
    I64 {
        value: i64,
        span: Span,
    },
    /// LuaJIT unsigned 64-bit integer literal, e.g. `0xffULL`.
    #[cfg(feature = "luajit")]
    U64 {
        value: u64,
        span: Span,
    },
    /// LuaJIT imaginary literal, e.g. `12.5i`.
    #[cfg(feature = "luajit")]
    Imaginary {
        value: f64,
        span: Span,
    },
    /// CfxLua compile-time jenkins hash string, e.g. `` `WEAPON_PISTOL` ``.
    /// The symbol is the raw literal including the backticks.
    #[cfg(feature = "cfxlua")]
    JenkinsHash {
        value: Symbol,
        span: Span,
    },
}

impl Spanned for Expression<'_> {
    fn span(&self) -> Span {
        match self {
            Expression::Nil { span, .. }
            | Expression::True { span, .. }
            | Expression::False { span, .. }
            | Expression::Vararg { span, .. }
            | Expression::Integer { span, .. }
            | Expression::Float { span, .. }
            | Expression::String { span, .. }
            | Expression::Identifier { span, .. }
            | Expression::Unary { span, .. }
            | Expression::Binary { span, .. }
            | Expression::Table { span, .. }
            | Expression::Closure { span, .. }
            | Expression::Call { span, .. }
            | Expression::MethodCall { span, .. }
            | Expression::Field { span, .. }
            | Expression::Index { span, .. }
            | Expression::Cast { span, .. }
            | Expression::Paren { span, .. } => *span,
            #[cfg(feature = "luau")]
            Expression::IfElse { span, .. } | Expression::InterpolatedString { span, .. } => *span,
            #[cfg(feature = "luajit")]
            Expression::I64 { span, .. }
            | Expression::U64 { span, .. }
            | Expression::Imaginary { span, .. } => *span,
            #[cfg(feature = "cfxlua")]
            Expression::JenkinsHash { span, .. } => *span,
        }
    }
}

/// One `elseif c then v` arm of a Luau `if` expression.
#[cfg(feature = "luau")]
#[derive(Debug, Clone)]
pub struct ElseIfExpression<'arena> {
    pub binding: Option<IfConditionBinding<'arena>>,
    pub condition: Expression<'arena>,
    pub result: Expression<'arena>,
}

/// A `local` or `const` binding whose initializer supplies an if condition.
#[cfg(feature = "luau")]
#[derive(Debug, Clone)]
pub struct IfConditionBinding<'arena> {
    pub variable: LocalVariable<'arena>,
    pub is_const: bool,
}

/// A literal chunk or `{expr}` hole of a Luau interpolated string. Literal
/// chunks are raw source text; [`crate::string::decode_escapes`] decodes them.
#[cfg(feature = "luau")]
#[derive(Debug, Clone)]
pub enum InterpolationPart<'arena> {
    Literal { value: Symbol, span: Span },
    Expression(Expression<'arena>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOperator {
    Negate,
    Not,
    Length,
    BitwiseNot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    FloorDivide,
    Modulo,
    Power,
    Concatenate,
    BitwiseAnd,
    BitwiseOr,
    BitwiseXor,
    ShiftLeft,
    ShiftRight,
    Equal,
    NotEqual,
    LessThan,
    GreaterThan,
    LessEqual,
    GreaterEqual,
    And,
    Or,
}

/// One entry of a table constructor.
#[derive(Debug, Clone)]
pub struct TableField<'arena> {
    pub key: TableKey<'arena>,
    pub value: Expression<'arena>,
}

#[derive(Debug, Clone)]
pub enum TableKey<'arena> {
    Expression(Expression<'arena>),
    Name { symbol: Symbol, span: Span },
    None,
}

/// The arguments of a call.
#[derive(Debug, Clone)]
pub enum CallArguments<'arena> {
    Expressions {
        type_arguments: Option<&'arena [TypeAnnotation<'arena>]>,
        arguments: &'arena [Expression<'arena>],
    },
    /// `value` is the literal as written, delimiters and escapes included;
    /// [`crate::string::decode_literal`] yields the bytes it denotes.
    String { value: Symbol, span: Span },
    Table {
        fields: &'arena [TableField<'arena>],
        span: Span,
    },
}

/// A function's signature and block. The span starts after the `function`
/// keyword and name, at the generic parameters or `(`.
#[derive(Debug, Clone)]
pub struct FunctionBody<'arena> {
    /// Luau function attributes (`@native function ...`); empty elsewhere.
    pub attributes: &'arena [SpannedSymbol],
    pub type_parameters: &'arena [TypeParameter<'arena>],
    pub parameters: &'arena [Parameter<'arena>],
    pub has_vararg: bool,
    /// Type annotation on the vararg (`...: T`); `None` elsewhere.
    pub vararg_type: Option<TypeAnnotation<'arena>>,
    pub returns: Option<&'arena [TypeAnnotation<'arena>]>,
    pub body: &'arena [Statement<'arena>],
    pub span: Span,
}

/// A declared generic type parameter: `T`, `T...` or `T = string`.
#[derive(Debug, Clone)]
pub struct TypeParameter<'arena> {
    pub symbol: Symbol,
    pub name_span: Span,
    /// Luau generic type pack (`T...`).
    pub is_pack: bool,
    /// Luau default value (`T = string`, `T... = ()`).
    pub default: Option<TypeAnnotation<'arena>>,
}

impl Spanned for FunctionBody<'_> {
    fn span(&self) -> Span {
        self.span
    }
}

/// A declared function parameter, or a `for ... in` loop variable.
#[derive(Debug, Clone)]
pub struct Parameter<'arena> {
    pub symbol: Symbol,
    pub name_span: Span,
    pub type_annotation: Option<TypeAnnotation<'arena>>,
}

impl Spanned for Parameter<'_> {
    fn span(&self) -> Span {
        match &self.type_annotation {
            Some(annotation) => Span::new(self.name_span.start, annotation.span().end),
            None => self.name_span,
        }
    }
}

#[allow(
    clippy::large_enum_variant,
    reason = "statements live in arena slices, so boxing would only add indirection"
)]
#[derive(Debug, Clone)]
pub enum Statement<'arena> {
    Local {
        variables: &'arena [LocalVariable<'arena>],
        values: &'arena [Expression<'arena>],
        span: Span,
    },
    LocalFunction {
        symbol: Symbol,
        name_span: Span,
        body: &'arena FunctionBody<'arena>,
        span: Span,
    },
    Assign {
        targets: &'arena [Expression<'arena>],
        values: &'arena [Expression<'arena>],
        span: Span,
    },
    Function {
        name: FunctionName<'arena>,
        body: &'arena FunctionBody<'arena>,
        span: Span,
    },
    Do {
        body: &'arena [Statement<'arena>],
        span: Span,
    },
    If {
        #[cfg(feature = "luau")]
        binding: Option<IfConditionBinding<'arena>>,
        condition: Expression<'arena>,
        then_block: &'arena [Statement<'arena>],
        elseifs: &'arena [ElseIf<'arena>],
        else_block: Option<&'arena [Statement<'arena>]>,
        span: Span,
    },
    While {
        condition: Expression<'arena>,
        body: &'arena [Statement<'arena>],
        span: Span,
    },
    Repeat {
        body: &'arena [Statement<'arena>],
        until: Expression<'arena>,
        span: Span,
    },
    NumericFor {
        variable: Symbol,
        variable_span: Span,
        /// Luau loop-variable annotation (`for i: number = ...`).
        variable_type: Option<TypeAnnotation<'arena>>,
        start: Expression<'arena>,
        limit: Expression<'arena>,
        step: Option<Expression<'arena>>,
        body: &'arena [Statement<'arena>],
        span: Span,
    },
    GenericFor {
        variables: &'arena [Parameter<'arena>],
        iterators: &'arena [Expression<'arena>],
        body: &'arena [Statement<'arena>],
        span: Span,
    },
    Return {
        values: &'arena [Expression<'arena>],
        span: Span,
    },
    Break {
        span: Span,
    },
    Goto {
        label: Symbol,
        label_span: Span,
        span: Span,
    },
    Label {
        symbol: Symbol,
        name_span: Span,
        span: Span,
    },
    Expression {
        expr: &'arena Expression<'arena>,
        span: Span,
    },
    /// Compound assignment, e.g. `a += 1`, `s ..= "x"` (Luau, CfxLua).
    #[cfg(feature = "compound")]
    CompoundAssign {
        target: Expression<'arena>,
        operator: CompoundOperator,
        value: Expression<'arena>,
        span: Span,
    },
    /// Luau `continue` statement.
    #[cfg(feature = "luau")]
    Continue {
        span: Span,
    },
    /// Luau type alias, e.g. `type Id = number` or `export type Id = number`.
    #[cfg(feature = "luau")]
    TypeAlias {
        export: bool,
        symbol: Symbol,
        name_span: Span,
        type_parameters: &'arena [TypeParameter<'arena>],
        value: TypeAnnotation<'arena>,
        span: Span,
    },
    /// Luau `const` declaration, e.g. `const x, y = 1, 2`.
    #[cfg(feature = "luau")]
    Const {
        variables: &'arena [LocalVariable<'arena>],
        values: &'arena [Expression<'arena>],
        span: Span,
    },
    /// Luau type function, e.g. `type function f(t) ... end`.
    #[cfg(feature = "luau")]
    TypeFunction {
        export: bool,
        symbol: Symbol,
        name_span: Span,
        body: &'arena FunctionBody<'arena>,
        span: Span,
    },
    /// CfxLua `local a, b in t`, reading each name as a field of `source`.
    #[cfg(feature = "cfxlua")]
    InUnpack {
        variables: &'arena [LocalVariable<'arena>],
        source: Expression<'arena>,
        span: Span,
    },
}

/// The operator of a compound assignment.
#[cfg(feature = "compound")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompoundOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    FloorDivide,
    Modulo,
    Power,
    Concatenate,
    #[cfg(feature = "cfxlua")]
    BitwiseAnd,
    #[cfg(feature = "cfxlua")]
    BitwiseOr,
    #[cfg(feature = "cfxlua")]
    ShiftLeft,
    #[cfg(feature = "cfxlua")]
    ShiftRight,
}

impl Spanned for Statement<'_> {
    fn span(&self) -> Span {
        match self {
            Statement::Local { span, .. }
            | Statement::LocalFunction { span, .. }
            | Statement::Assign { span, .. }
            | Statement::Function { span, .. }
            | Statement::Do { span, .. }
            | Statement::If { span, .. }
            | Statement::While { span, .. }
            | Statement::Repeat { span, .. }
            | Statement::NumericFor { span, .. }
            | Statement::GenericFor { span, .. }
            | Statement::Return { span, .. }
            | Statement::Break { span, .. }
            | Statement::Goto { span, .. }
            | Statement::Label { span, .. }
            | Statement::Expression { span, .. } => *span,
            #[cfg(feature = "compound")]
            Statement::CompoundAssign { span, .. } => *span,
            #[cfg(feature = "luau")]
            Statement::Continue { span, .. }
            | Statement::TypeAlias { span, .. }
            | Statement::Const { span, .. }
            | Statement::TypeFunction { span, .. } => *span,
            #[cfg(feature = "cfxlua")]
            Statement::InUnpack { span, .. } => *span,
        }
    }
}

/// One name bound by `local`, Luau `const` or CfxLua `local ... in`.
#[derive(Debug, Clone)]
pub struct LocalVariable<'arena> {
    pub symbol: Symbol,
    pub name_span: Span,
    pub type_annotation: Option<TypeAnnotation<'arena>>,
    pub attribute: Option<VariableAttribute>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VariableAttribute {
    Close,
    Const,
}

/// One `elseif c then ...` arm of an `if` statement.
#[derive(Debug, Clone)]
pub struct ElseIf<'arena> {
    #[cfg(feature = "luau")]
    pub binding: Option<IfConditionBinding<'arena>>,
    pub condition: Expression<'arena>,
    pub body: &'arena [Statement<'arena>],
}

/// The dotted name of a `function a.b.c:d()` declaration.
#[derive(Debug, Clone)]
pub struct FunctionName<'arena> {
    pub path: &'arena [SpannedSymbol],
    pub method: Option<SpannedSymbol>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpannedSymbol {
    pub symbol: Symbol,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypeAnnotation<'arena> {
    Named {
        name: Symbol,
        name_span: Span,
    },
    Array {
        element: &'arena TypeAnnotation<'arena>,
        span: Span,
    },
    Map {
        key: &'arena TypeAnnotation<'arena>,
        value: &'arena TypeAnnotation<'arena>,
        span: Span,
    },
    Tuple {
        elements: &'arena [TypeAnnotation<'arena>],
        span: Span,
    },
    Function {
        type_parameters: &'arena [TypeParameter<'arena>],
        parameters: &'arena [FunctionTypeParameter<'arena>],
        returns: &'arena [TypeAnnotation<'arena>],
        span: Span,
    },
    Generic {
        name: Symbol,
        name_span: Span,
        arguments: &'arena [TypeAnnotation<'arena>],
        span: Span,
    },
    Union {
        variants: &'arena [TypeAnnotation<'arena>],
        span: Span,
    },
    Optional {
        inner: &'arena TypeAnnotation<'arena>,
        span: Span,
    },
    /// Luau intersection type, e.g. `A & B`.
    #[cfg(feature = "luau")]
    Intersection {
        parts: &'arena [TypeAnnotation<'arena>],
        span: Span,
    },
    /// Luau `typeof(expr)` type.
    #[cfg(feature = "luau")]
    Typeof {
        expression: &'arena Expression<'arena>,
        span: Span,
    },
    /// Luau singleton type, e.g. the string `"ok"` or `true` used as a type.
    #[cfg(feature = "luau")]
    Singleton {
        value: SingletonValue,
        span: Span,
    },
    /// Luau variadic type pack, e.g. `...number`.
    #[cfg(feature = "luau")]
    Variadic {
        inner: &'arena TypeAnnotation<'arena>,
        span: Span,
    },
    /// Luau generic type pack reference, e.g. `T...`.
    #[cfg(feature = "luau")]
    Pack {
        name: Symbol,
        span: Span,
    },
    /// Luau qualified type from a module, e.g. `module.Foo<string>`.
    #[cfg(feature = "luau")]
    Qualified {
        module: SpannedSymbol,
        name: SpannedSymbol,
        /// `Some` when type arguments are given (`module.Foo<...>`, possibly
        /// empty: `module.Foo<>`).
        arguments: Option<&'arena [TypeAnnotation<'arena>]>,
        span: Span,
    },
    /// Luau table type with named fields and/or indexers, e.g.
    /// `{ read x: number, [string]: boolean }`.
    #[cfg(feature = "luau")]
    Struct {
        fields: &'arena [TableTypeField<'arena>],
        span: Span,
    },
}

/// One field of a Luau [`TypeAnnotation::Struct`] table type.
#[cfg(feature = "luau")]
#[derive(Debug, Clone)]
pub struct TableTypeField<'arena> {
    pub access: Option<TableAccess>,
    pub key: TableTypeKey<'arena>,
    pub value: TypeAnnotation<'arena>,
}

/// The key of a Luau table type field.
#[cfg(feature = "luau")]
#[derive(Debug, Clone)]
pub enum TableTypeKey<'arena> {
    /// `name: T`
    Name(SpannedSymbol),
    /// `[K]: V`
    Indexer(&'arena TypeAnnotation<'arena>),
}

/// Luau table type access modifier (`read` / `write`).
#[cfg(feature = "luau")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TableAccess {
    Read,
    Write,
}

/// A parameter of a function type: optionally named (`(cost: number) -> ()`).
#[derive(Debug, Clone)]
pub struct FunctionTypeParameter<'arena> {
    pub name: Option<SpannedSymbol>,
    pub type_annotation: TypeAnnotation<'arena>,
}

/// The value of a Luau singleton type.
#[cfg(feature = "luau")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SingletonValue {
    String(Symbol),
    Boolean(bool),
}

impl Spanned for TypeAnnotation<'_> {
    fn span(&self) -> Span {
        match self {
            TypeAnnotation::Named { name_span, .. } => *name_span,
            TypeAnnotation::Array { span, .. }
            | TypeAnnotation::Map { span, .. }
            | TypeAnnotation::Tuple { span, .. }
            | TypeAnnotation::Function { span, .. }
            | TypeAnnotation::Generic { span, .. }
            | TypeAnnotation::Union { span, .. }
            | TypeAnnotation::Optional { span, .. } => *span,
            #[cfg(feature = "luau")]
            TypeAnnotation::Intersection { span, .. }
            | TypeAnnotation::Typeof { span, .. }
            | TypeAnnotation::Singleton { span, .. }
            | TypeAnnotation::Variadic { span, .. }
            | TypeAnnotation::Pack { span, .. }
            | TypeAnnotation::Qualified { span, .. }
            | TypeAnnotation::Struct { span, .. } => *span,
        }
    }
}
