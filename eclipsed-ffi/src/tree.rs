//! The syntax tree as C types.
//!
//! Every node is a `kind`, a `span` and a `data` union whose active member
//! is named after the kind. Children are pointers, lists are `{ptr, len}`,
//! absent children are null pointers and absent names are empty ranges.
//! All of it lives in the owning [`crate::EclChunk`]'s arena.

use std::ffi::c_char;
use std::ptr;

use bumpalo::Bump;
use eclipsed::ast;
use eclipsed::lexer::Span;
use eclipsed::parser::ensure_stack;
use eclipsed::string;

/// A byte range `[start, end)` into the source.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EclRange {
    pub start: u32,
    pub end: u32,
}

impl From<Span> for EclRange {
    fn from(span: Span) -> Self {
        EclRange {
            start: span.start,
            end: span.end,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclList<T> {
    pub ptr: *const T,
    pub len: usize,
}

impl<T> EclList<T> {
    const EMPTY: Self = EclList {
        ptr: ptr::null(),
        len: 0,
    };

    /// # Safety
    ///
    /// A non-empty list must point to `len` live items.
    pub(crate) unsafe fn as_slice(&self) -> &[T] {
        if self.len == 0 {
            &[]
        } else {
            // SAFETY: guaranteed by the caller.
            unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
        }
    }
}

/// Decoded bytes; `ptr[len]` is a NUL byte.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclBytes {
    pub ptr: *const c_char,
    pub len: usize,
}

impl EclBytes {
    const EMPTY: Self = EclBytes {
        ptr: c"".as_ptr(),
        len: 0,
    };
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EclExprKind {
    Nil = 0,
    True = 1,
    False = 2,
    Integer = 3,
    Float = 4,
    String = 5,
    Vararg = 6,
    Name = 7,
    Unary = 8,
    Binary = 9,
    Table = 10,
    Function = 11,
    Call = 12,
    MethodCall = 13,
    Field = 14,
    Index = 15,
    Cast = 16,
    Paren = 17,
    IfElse = 18,
    Interpolated = 19,
    /// LuaJIT `1LL`.
    I64 = 20,
    /// LuaJIT `1ULL`.
    U64 = 21,
    /// LuaJIT `1i`.
    Imaginary = 22,
    /// CfxLua `` `name` ``; the text is the span.
    JenkinsHash = 23,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EclUnaryOp {
    Negate = 0,
    Not = 1,
    Length = 2,
    BitwiseNot = 3,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EclBinaryOp {
    Add = 0,
    Subtract = 1,
    Multiply = 2,
    Divide = 3,
    FloorDivide = 4,
    Modulo = 5,
    Power = 6,
    Concatenate = 7,
    BitwiseAnd = 8,
    BitwiseOr = 9,
    BitwiseXor = 10,
    ShiftLeft = 11,
    ShiftRight = 12,
    Equal = 13,
    NotEqual = 14,
    LessThan = 15,
    GreaterThan = 16,
    LessEqual = 17,
    GreaterEqual = 18,
    And = 19,
    Or = 20,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclExpr {
    pub kind: EclExprKind,
    pub span: EclRange,
    pub data: EclExprData,
}

/// The member named after the node's kind is active. `NIL`, `TRUE`,
/// `FALSE`, `VARARG`, `NAME` and `JENKINS_HASH` carry nothing beyond the span.
#[repr(C)]
#[derive(Clone, Copy)]
pub union EclExprData {
    pub none: u8,
    /// `INTEGER` and `I64`.
    pub integer: i64,
    pub u64: u64,
    /// `FLOAT` and `IMAGINARY`.
    pub number: f64,
    pub string: EclBytes,
    pub unary: EclUnary,
    pub binary: EclBinary,
    pub table: EclList<EclField>,
    pub function: *const EclFunction,
    pub call: EclCall,
    pub method_call: EclMethodCall,
    pub field: EclFieldAccess,
    pub index: EclIndex,
    pub cast: EclCast,
    pub paren: *const EclExpr,
    pub if_else: EclIfElse,
    pub interpolated: EclList<EclInterpolation>,
}

impl std::fmt::Debug for EclExprData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EclExprData { .. }")
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclUnary {
    pub op: EclUnaryOp,
    pub operand: *const EclExpr,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclBinary {
    pub op: EclBinaryOp,
    pub left: *const EclExpr,
    pub right: *const EclExpr,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EclArgumentsKind {
    /// `f(a, b)`.
    List = 0,
    /// `f"s"`: `values` holds the one string.
    String = 1,
    /// `f{...}`: `values` holds the one table.
    Table = 2,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclArguments {
    pub kind: EclArgumentsKind,
    /// Luau `f<<T>>(...)`.
    pub type_arguments: EclList<EclType>,
    pub values: EclList<EclExpr>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclCall {
    pub callee: *const EclExpr,
    pub arguments: EclArguments,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclMethodCall {
    pub receiver: *const EclExpr,
    pub method: EclRange,
    pub arguments: EclArguments,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclFieldAccess {
    pub table: *const EclExpr,
    pub name: EclRange,
    /// CfxLua `t?.name`.
    pub safe: bool,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclIndex {
    pub table: *const EclExpr,
    pub key: *const EclExpr,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclCast {
    pub expression: *const EclExpr,
    pub target: *const EclType,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclIfElse {
    pub binding: EclIfBinding,
    pub condition: *const EclExpr,
    pub then_branch: *const EclExpr,
    pub elseifs: EclList<EclElseIfExpr>,
    pub else_branch: *const EclExpr,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclElseIfExpr {
    pub binding: EclIfBinding,
    pub condition: *const EclExpr,
    pub result: *const EclExpr,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EclFieldKind {
    /// `{ value }`.
    Positional = 0,
    /// `{ name = value }`; CfxLua `{ .name }` is one with a synthesized `true`.
    Named = 1,
    /// `{ [key] = value }`.
    Keyed = 2,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclField {
    pub kind: EclFieldKind,
    pub name: EclRange,
    pub key: *const EclExpr,
    pub value: *const EclExpr,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclInterpolation {
    /// Null for a literal chunk.
    pub expression: *const EclExpr,
    /// The literal chunk as written.
    pub literal: EclRange,
    /// The literal chunk decoded.
    pub text: EclBytes,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclFunction {
    /// From the generic parameters or `(` through `end`.
    pub span: EclRange,
    /// Luau `@name`s.
    pub attributes: EclList<EclRange>,
    pub type_parameters: EclList<EclTypeParameter>,
    pub parameters: EclList<EclParameter>,
    pub vararg: bool,
    pub vararg_type: *const EclType,
    /// Whether a return annotation is written, which may be empty: `(): ()`.
    pub has_returns: bool,
    pub returns: EclList<EclType>,
    pub body: EclList<EclStmt>,
}

/// A function parameter or a `for` variable.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclParameter {
    pub name: EclRange,
    pub type_annotation: *const EclType,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EclAttribute {
    None = 0,
    Const = 1,
    Close = 2,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclLocal {
    pub name: EclRange,
    pub attribute: EclAttribute,
    pub type_annotation: *const EclType,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclIfBinding {
    pub present: bool,
    pub is_const: bool,
    pub variable: EclIfVariable,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclIfVariable {
    pub name: EclRange,
    pub attribute: EclAttribute,
    pub type_annotation: *const EclType,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EclStmtKind {
    Local = 0,
    LocalFunction = 1,
    Assign = 2,
    Function = 3,
    Do = 4,
    If = 5,
    While = 6,
    Repeat = 7,
    NumericFor = 8,
    GenericFor = 9,
    Return = 10,
    Break = 11,
    Goto = 12,
    Label = 13,
    /// A call used as a statement.
    Call = 14,
    CompoundAssign = 15,
    Continue = 16,
    TypeAlias = 17,
    Const = 18,
    TypeFunction = 19,
    /// CfxLua `local a, b in t`.
    InUnpack = 20,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EclCompoundOp {
    Add = 0,
    Subtract = 1,
    Multiply = 2,
    Divide = 3,
    FloorDivide = 4,
    Modulo = 5,
    Power = 6,
    Concatenate = 7,
    BitwiseAnd = 8,
    BitwiseOr = 9,
    ShiftLeft = 10,
    ShiftRight = 11,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclStmt {
    pub kind: EclStmtKind,
    pub span: EclRange,
    pub data: EclStmtData,
}

/// The member named after the node's kind is active; `LOCAL`, `CONST` and
/// `IN_UNPACK` use `local`, `WHILE` and `REPEAT` use `loop`, `GOTO` and
/// `LABEL` use `label`. `BREAK` and `CONTINUE` carry nothing.
#[repr(C)]
#[derive(Clone, Copy)]
pub union EclStmtData {
    pub none: u8,
    pub local: EclLocalStmt,
    pub local_function: EclLocalFunction,
    pub assign: EclAssign,
    pub function: EclFunctionStmt,
    pub block: EclList<EclStmt>,
    pub if_: EclIf,
    pub loop_: EclLoop,
    pub numeric_for: EclNumericFor,
    pub generic_for: EclGenericFor,
    pub return_: EclList<EclExpr>,
    pub label: EclRange,
    pub call: *const EclExpr,
    pub compound: EclCompound,
    pub type_alias: EclTypeAlias,
    pub type_function: EclTypeFunction,
}

impl std::fmt::Debug for EclStmtData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EclStmtData { .. }")
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclLocalStmt {
    pub variables: EclList<EclLocal>,
    /// For `IN_UNPACK`, the one source table.
    pub values: EclList<EclExpr>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclLocalFunction {
    pub name: EclRange,
    pub body: *const EclFunction,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclAssign {
    pub targets: EclList<EclExpr>,
    pub values: EclList<EclExpr>,
}

/// `function a.b.c:d() ... end`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclFunctionStmt {
    pub path: EclList<EclRange>,
    /// Empty unless declared with `:`.
    pub method: EclRange,
    pub body: *const EclFunction,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclIf {
    pub binding: EclIfBinding,
    pub condition: *const EclExpr,
    pub then_block: EclList<EclStmt>,
    pub elseifs: EclList<EclElseIf>,
    pub has_else: bool,
    pub else_block: EclList<EclStmt>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclElseIf {
    pub binding: EclIfBinding,
    pub condition: *const EclExpr,
    pub body: EclList<EclStmt>,
}

/// `while condition do body end`, or `repeat body until condition`, where
/// the condition sees the body's locals.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclLoop {
    pub condition: *const EclExpr,
    pub body: EclList<EclStmt>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclNumericFor {
    pub variable: EclParameter,
    pub start: *const EclExpr,
    pub limit: *const EclExpr,
    pub step: *const EclExpr,
    pub body: EclList<EclStmt>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclGenericFor {
    pub variables: EclList<EclParameter>,
    pub iterators: EclList<EclExpr>,
    pub body: EclList<EclStmt>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclCompound {
    pub target: *const EclExpr,
    pub op: EclCompoundOp,
    pub value: *const EclExpr,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclTypeAlias {
    pub exported: bool,
    pub name: EclRange,
    pub type_parameters: EclList<EclTypeParameter>,
    pub value: *const EclType,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclTypeFunction {
    pub exported: bool,
    pub name: EclRange,
    pub body: *const EclFunction,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EclTypeKind {
    /// `T`; the name is the span.
    Named = 0,
    /// `{ T }`.
    Array = 1,
    /// `{ [K]: V }`.
    Map = 2,
    /// `(T, U)` where a pack is allowed.
    Tuple = 3,
    Function = 4,
    /// `T<U>`.
    Generic = 5,
    Union = 6,
    Optional = 7,
    Intersection = 8,
    Typeof = 9,
    Singleton = 10,
    /// `...T`.
    Variadic = 11,
    /// `T...`.
    Pack = 12,
    /// `module.T` or `module.T<U>`.
    Qualified = 13,
    /// `{ name: T, [K]: V }`.
    Struct = 14,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclType {
    pub kind: EclTypeKind,
    pub span: EclRange,
    pub data: EclTypeData,
}

/// `ARRAY`, `OPTIONAL` and `VARIADIC` use `inner`; `TUPLE`, `UNION` and
/// `INTERSECTION` use `list`; `GENERIC` and `QUALIFIED` use `generic`;
/// `PACK` uses `name`. `NAMED` carries nothing beyond the span.
#[repr(C)]
#[derive(Clone, Copy)]
pub union EclTypeData {
    pub none: u8,
    pub inner: *const EclType,
    pub map: EclMapType,
    pub list: EclList<EclType>,
    pub function: EclFunctionType,
    pub generic: EclGenericType,
    pub typeof_: *const EclExpr,
    pub singleton: EclSingleton,
    pub name: EclRange,
    pub struct_: EclList<EclTableTypeField>,
}

impl std::fmt::Debug for EclTypeData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EclTypeData { .. }")
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclMapType {
    pub key: *const EclType,
    pub value: *const EclType,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclFunctionType {
    pub type_parameters: EclList<EclTypeParameter>,
    pub parameters: EclList<EclFunctionTypeParameter>,
    pub returns: EclList<EclType>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclFunctionTypeParameter {
    /// Empty when unnamed.
    pub name: EclRange,
    pub type_: *const EclType,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclGenericType {
    /// Empty unless `QUALIFIED`.
    pub module: EclRange,
    pub name: EclRange,
    /// Whether `<...>` is written, which may be empty.
    pub has_arguments: bool,
    pub arguments: EclList<EclType>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclSingleton {
    /// A string singleton when true, else a boolean one.
    pub is_string: bool,
    pub boolean: bool,
    pub string: EclBytes,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclTypeParameter {
    pub name: EclRange,
    /// `T...`.
    pub pack: bool,
    pub default_type: *const EclType,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EclTableTypeAccess {
    None = 0,
    Read = 1,
    Write = 2,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EclTableTypeField {
    pub access: EclTableTypeAccess,
    /// Empty for an indexer.
    pub name: EclRange,
    /// Null for a named field.
    pub indexer: *const EclType,
    pub value: *const EclType,
}

/// Converts the core tree into the C types, allocating into `arena`.
pub(crate) struct Builder<'a> {
    pub(crate) arena: &'a Bump,
    pub(crate) source: &'a str,
}

impl Builder<'_> {
    fn if_binding(&self, binding: Option<&ast::IfConditionBinding<'_>>) -> EclIfBinding {
        EclIfBinding {
            present: binding.is_some(),
            is_const: binding.is_some_and(|binding| binding.is_const),
            variable: EclIfVariable {
                name: binding.map_or(EclRange::default(), |binding| {
                    binding.variable.name_span.into()
                }),
                attribute: EclAttribute::None,
                type_annotation: self.opt_type(
                    binding.and_then(|binding| binding.variable.type_annotation.as_ref()),
                ),
            },
        }
    }

    fn list<T, U>(&self, items: &[T], mut convert: impl FnMut(&T) -> U) -> EclList<U> {
        if items.is_empty() {
            return EclList::EMPTY;
        }
        let slice = self
            .arena
            .alloc_slice_fill_iter(items.iter().map(&mut convert));
        EclList {
            ptr: slice.as_ptr(),
            len: slice.len(),
        }
    }

    fn boxed<T>(&self, value: T) -> *const T {
        self.arena.alloc(value)
    }

    fn bytes(&self, bytes: &[u8]) -> EclBytes {
        let slice = self.arena.alloc_slice_fill_default(bytes.len() + 1);
        slice[..bytes.len()].copy_from_slice(bytes);
        EclBytes {
            ptr: slice.as_ptr().cast(),
            len: bytes.len(),
        }
    }

    fn text(&self, span: Span) -> &str {
        &self.source[span.range()]
    }

    pub(crate) fn block(&self, statements: &[ast::Statement<'_>]) -> EclList<EclStmt> {
        self.list(statements, |statement| self.statement(statement))
    }

    fn expr_ptr(&self, expression: &ast::Expression<'_>) -> *const EclExpr {
        self.boxed(self.expression(expression))
    }

    fn opt_expr(&self, expression: Option<&ast::Expression<'_>>) -> *const EclExpr {
        expression.map_or(ptr::null(), |e| self.expr_ptr(e))
    }

    fn type_ptr(&self, annotation: &ast::TypeAnnotation<'_>) -> *const EclType {
        self.boxed(self.annotation(annotation))
    }

    fn opt_type(&self, annotation: Option<&ast::TypeAnnotation<'_>>) -> *const EclType {
        annotation.map_or(ptr::null(), |a| self.type_ptr(a))
    }

    fn types(&self, annotations: &[ast::TypeAnnotation<'_>]) -> EclList<EclType> {
        self.list(annotations, |a| self.annotation(a))
    }

    fn expressions(&self, expressions: &[ast::Expression<'_>]) -> EclList<EclExpr> {
        self.list(expressions, |e| self.expression(e))
    }

    fn expression(&self, expression: &ast::Expression<'_>) -> EclExpr {
        ensure_stack(|| self.expression_node(expression))
    }

    fn expression_node(&self, expression: &ast::Expression<'_>) -> EclExpr {
        use ast::Expression as E;
        use ast::Spanned;

        let span = expression.span();
        let (kind, data) = match expression {
            E::Nil { .. } => (EclExprKind::Nil, EclExprData { none: 0 }),
            E::True { .. } => (EclExprKind::True, EclExprData { none: 0 }),
            E::False { .. } => (EclExprKind::False, EclExprData { none: 0 }),
            E::Vararg { .. } => (EclExprKind::Vararg, EclExprData { none: 0 }),
            E::Identifier { .. } => (EclExprKind::Name, EclExprData { none: 0 }),
            E::Integer { value, .. } => (EclExprKind::Integer, EclExprData { integer: *value }),
            E::Float { value, .. } => (EclExprKind::Float, EclExprData { number: *value }),
            E::String { span, .. } => (
                EclExprKind::String,
                EclExprData {
                    string: self.bytes(&string::decode_literal(self.text(*span))),
                },
            ),
            E::Unary {
                operator, operand, ..
            } => (
                EclExprKind::Unary,
                EclExprData {
                    unary: EclUnary {
                        op: unary_op(*operator),
                        operand: self.expr_ptr(operand),
                    },
                },
            ),
            E::Binary {
                left,
                operator,
                right,
                ..
            } => (
                EclExprKind::Binary,
                EclExprData {
                    binary: EclBinary {
                        op: binary_op(*operator),
                        left: self.expr_ptr(left),
                        right: self.expr_ptr(right),
                    },
                },
            ),
            E::Table { fields, .. } => (
                EclExprKind::Table,
                EclExprData {
                    table: self.fields(fields),
                },
            ),
            E::Closure { body, .. } => (
                EclExprKind::Function,
                EclExprData {
                    function: self.function(body),
                },
            ),
            E::Call {
                callee, arguments, ..
            } => (
                EclExprKind::Call,
                EclExprData {
                    call: EclCall {
                        callee: self.expr_ptr(callee),
                        arguments: self.arguments(arguments),
                    },
                },
            ),
            E::MethodCall {
                receiver,
                method_span,
                arguments,
                ..
            } => (
                EclExprKind::MethodCall,
                EclExprData {
                    method_call: EclMethodCall {
                        receiver: self.expr_ptr(receiver),
                        method: (*method_span).into(),
                        arguments: self.arguments(arguments),
                    },
                },
            ),
            E::Field {
                table,
                field_span,
                safe,
                ..
            } => (
                EclExprKind::Field,
                EclExprData {
                    field: EclFieldAccess {
                        table: self.expr_ptr(table),
                        name: (*field_span).into(),
                        safe: *safe,
                    },
                },
            ),
            E::Index { table, key, .. } => (
                EclExprKind::Index,
                EclExprData {
                    index: EclIndex {
                        table: self.expr_ptr(table),
                        key: self.expr_ptr(key),
                    },
                },
            ),
            E::Cast {
                expression, target, ..
            } => (
                EclExprKind::Cast,
                EclExprData {
                    cast: EclCast {
                        expression: self.expr_ptr(expression),
                        target: self.type_ptr(target),
                    },
                },
            ),
            E::Paren { expression, .. } => (
                EclExprKind::Paren,
                EclExprData {
                    paren: self.expr_ptr(expression),
                },
            ),
            E::IfElse {
                binding,
                condition,
                then_branch,
                elseifs,
                else_branch,
                ..
            } => (
                EclExprKind::IfElse,
                EclExprData {
                    if_else: EclIfElse {
                        binding: self.if_binding(binding.as_ref()),
                        condition: self.expr_ptr(condition),
                        then_branch: self.expr_ptr(then_branch),
                        elseifs: self.list(elseifs, |arm| EclElseIfExpr {
                            binding: self.if_binding(arm.binding.as_ref()),
                            condition: self.expr_ptr(&arm.condition),
                            result: self.expr_ptr(&arm.result),
                        }),
                        else_branch: self.expr_ptr(else_branch),
                    },
                },
            ),
            E::InterpolatedString { parts, .. } => (
                EclExprKind::Interpolated,
                EclExprData {
                    interpolated: self.interpolation(parts),
                },
            ),
            E::I64 { value, .. } => (EclExprKind::I64, EclExprData { integer: *value }),
            E::U64 { value, .. } => (EclExprKind::U64, EclExprData { u64: *value }),
            E::Imaginary { value, .. } => (EclExprKind::Imaginary, EclExprData { number: *value }),
            E::JenkinsHash { .. } => (EclExprKind::JenkinsHash, EclExprData { none: 0 }),
        };
        EclExpr {
            kind,
            span: span.into(),
            data,
        }
    }

    fn interpolation(&self, parts: &[ast::InterpolationPart<'_>]) -> EclList<EclInterpolation> {
        self.list(parts, |part| match part {
            ast::InterpolationPart::Literal { span, .. } => EclInterpolation {
                expression: ptr::null(),
                literal: (*span).into(),
                text: self.bytes(&string::decode_escapes(self.text(*span).as_bytes())),
            },
            ast::InterpolationPart::Expression(expression) => EclInterpolation {
                expression: self.expr_ptr(expression),
                literal: EclRange::default(),
                text: EclBytes::EMPTY,
            },
        })
    }

    fn arguments(&self, arguments: &ast::CallArguments<'_>) -> EclArguments {
        match arguments {
            ast::CallArguments::Expressions {
                type_arguments,
                arguments,
            } => EclArguments {
                kind: EclArgumentsKind::List,
                type_arguments: type_arguments.map_or(EclList::EMPTY, |types| self.types(types)),
                values: self.expressions(arguments),
            },
            ast::CallArguments::String { value, span } => EclArguments {
                kind: EclArgumentsKind::String,
                type_arguments: EclList::EMPTY,
                values: self.expressions(&[ast::Expression::String {
                    value: *value,
                    span: *span,
                }]),
            },
            ast::CallArguments::Table { fields, span } => EclArguments {
                kind: EclArgumentsKind::Table,
                type_arguments: EclList::EMPTY,
                values: self.expressions(&[ast::Expression::Table {
                    fields,
                    span: *span,
                }]),
            },
        }
    }

    fn fields(&self, fields: &[ast::TableField<'_>]) -> EclList<EclField> {
        self.list(fields, |field| {
            let value = self.expr_ptr(&field.value);
            match &field.key {
                ast::TableKey::None => EclField {
                    kind: EclFieldKind::Positional,
                    name: EclRange::default(),
                    key: ptr::null(),
                    value,
                },
                ast::TableKey::Name { span, .. } => EclField {
                    kind: EclFieldKind::Named,
                    name: (*span).into(),
                    key: ptr::null(),
                    value,
                },
                ast::TableKey::Expression(key) => EclField {
                    kind: EclFieldKind::Keyed,
                    name: EclRange::default(),
                    key: self.expr_ptr(key),
                    value,
                },
            }
        })
    }

    fn function(&self, body: &ast::FunctionBody<'_>) -> *const EclFunction {
        self.boxed(EclFunction {
            span: body.span.into(),
            attributes: self.list(body.attributes, |a| a.span.into()),
            type_parameters: self.type_parameters(body.type_parameters),
            parameters: self.list(body.parameters, |p| self.parameter(p)),
            vararg: body.has_vararg,
            vararg_type: self.opt_type(body.vararg_type.as_ref()),
            has_returns: body.returns.is_some(),
            returns: body.returns.map_or(EclList::EMPTY, |r| self.types(r)),
            body: self.block(body.body),
        })
    }

    fn parameter(&self, parameter: &ast::Parameter<'_>) -> EclParameter {
        EclParameter {
            name: parameter.name_span.into(),
            type_annotation: self.opt_type(parameter.type_annotation.as_ref()),
        }
    }

    fn locals(&self, variables: &[ast::LocalVariable<'_>]) -> EclList<EclLocal> {
        self.list(variables, |variable| self.local(variable))
    }

    fn local(&self, variable: &ast::LocalVariable<'_>) -> EclLocal {
        EclLocal {
            name: variable.name_span.into(),
            attribute: match variable.attribute {
                None => EclAttribute::None,
                Some(ast::VariableAttribute::Const) => EclAttribute::Const,
                Some(ast::VariableAttribute::Close) => EclAttribute::Close,
            },
            type_annotation: self.opt_type(variable.type_annotation.as_ref()),
        }
    }

    fn type_parameters(&self, parameters: &[ast::TypeParameter<'_>]) -> EclList<EclTypeParameter> {
        self.list(parameters, |parameter| EclTypeParameter {
            name: parameter.name_span.into(),
            pack: parameter.is_pack,
            default_type: self.opt_type(parameter.default.as_ref()),
        })
    }

    fn statement(&self, statement: &ast::Statement<'_>) -> EclStmt {
        ensure_stack(|| self.statement_node(statement))
    }

    fn statement_node(&self, statement: &ast::Statement<'_>) -> EclStmt {
        use ast::Spanned;
        use ast::Statement as S;

        let local = |variables, values| EclStmtData {
            local: EclLocalStmt {
                variables: self.locals(variables),
                values: self.expressions(values),
            },
        };
        let (kind, data) = match statement {
            S::Local {
                variables, values, ..
            } => (EclStmtKind::Local, local(variables, values)),
            S::Const {
                variables, values, ..
            } => (EclStmtKind::Const, local(variables, values)),
            S::InUnpack {
                variables, source, ..
            } => (
                EclStmtKind::InUnpack,
                local(variables, std::slice::from_ref(source)),
            ),
            S::LocalFunction {
                name_span, body, ..
            } => (
                EclStmtKind::LocalFunction,
                EclStmtData {
                    local_function: EclLocalFunction {
                        name: (*name_span).into(),
                        body: self.function(body),
                    },
                },
            ),
            S::Assign {
                targets, values, ..
            } => (
                EclStmtKind::Assign,
                EclStmtData {
                    assign: EclAssign {
                        targets: self.expressions(targets),
                        values: self.expressions(values),
                    },
                },
            ),
            S::Function { name, body, .. } => (
                EclStmtKind::Function,
                EclStmtData {
                    function: EclFunctionStmt {
                        path: self.list(name.path, |part| part.span.into()),
                        method: name.method.map_or(EclRange::default(), |m| m.span.into()),
                        body: self.function(body),
                    },
                },
            ),
            S::Do { body, .. } => (
                EclStmtKind::Do,
                EclStmtData {
                    block: self.block(body),
                },
            ),
            S::If {
                binding,
                condition,
                then_block,
                elseifs,
                else_block,
                ..
            } => (
                EclStmtKind::If,
                EclStmtData {
                    if_: EclIf {
                        binding: self.if_binding(binding.as_ref()),
                        condition: self.expr_ptr(condition),
                        then_block: self.block(then_block),
                        elseifs: self.list(elseifs, |arm| EclElseIf {
                            binding: self.if_binding(arm.binding.as_ref()),
                            condition: self.expr_ptr(&arm.condition),
                            body: self.block(arm.body),
                        }),
                        has_else: else_block.is_some(),
                        else_block: else_block.map_or(EclList::EMPTY, |block| self.block(block)),
                    },
                },
            ),
            S::While {
                condition, body, ..
            } => (
                EclStmtKind::While,
                EclStmtData {
                    loop_: EclLoop {
                        condition: self.expr_ptr(condition),
                        body: self.block(body),
                    },
                },
            ),
            S::Repeat { body, until, .. } => (
                EclStmtKind::Repeat,
                EclStmtData {
                    loop_: EclLoop {
                        condition: self.expr_ptr(until),
                        body: self.block(body),
                    },
                },
            ),
            S::NumericFor {
                variable_span,
                variable_type,
                start,
                limit,
                step,
                body,
                ..
            } => (
                EclStmtKind::NumericFor,
                EclStmtData {
                    numeric_for: EclNumericFor {
                        variable: EclParameter {
                            name: (*variable_span).into(),
                            type_annotation: self.opt_type(variable_type.as_ref()),
                        },
                        start: self.expr_ptr(start),
                        limit: self.expr_ptr(limit),
                        step: self.opt_expr(step.as_ref()),
                        body: self.block(body),
                    },
                },
            ),
            S::GenericFor {
                variables,
                iterators,
                body,
                ..
            } => (
                EclStmtKind::GenericFor,
                EclStmtData {
                    generic_for: EclGenericFor {
                        variables: self.list(variables, |p| self.parameter(p)),
                        iterators: self.expressions(iterators),
                        body: self.block(body),
                    },
                },
            ),
            S::Return { values, .. } => (
                EclStmtKind::Return,
                EclStmtData {
                    return_: self.expressions(values),
                },
            ),
            S::Break { .. } => (EclStmtKind::Break, EclStmtData { none: 0 }),
            S::Continue { .. } => (EclStmtKind::Continue, EclStmtData { none: 0 }),
            S::Goto { label_span, .. } => (
                EclStmtKind::Goto,
                EclStmtData {
                    label: (*label_span).into(),
                },
            ),
            S::Label { name_span, .. } => (
                EclStmtKind::Label,
                EclStmtData {
                    label: (*name_span).into(),
                },
            ),
            S::Expression { expr, .. } => (
                EclStmtKind::Call,
                EclStmtData {
                    call: self.expr_ptr(expr),
                },
            ),
            S::CompoundAssign {
                target,
                operator,
                value,
                ..
            } => (
                EclStmtKind::CompoundAssign,
                EclStmtData {
                    compound: EclCompound {
                        target: self.expr_ptr(target),
                        op: compound_op(*operator),
                        value: self.expr_ptr(value),
                    },
                },
            ),
            S::TypeAlias {
                export,
                name_span,
                type_parameters,
                value,
                ..
            } => (
                EclStmtKind::TypeAlias,
                EclStmtData {
                    type_alias: EclTypeAlias {
                        exported: *export,
                        name: (*name_span).into(),
                        type_parameters: self.type_parameters(type_parameters),
                        value: self.type_ptr(value),
                    },
                },
            ),
            S::TypeFunction {
                export,
                name_span,
                body,
                ..
            } => (
                EclStmtKind::TypeFunction,
                EclStmtData {
                    type_function: EclTypeFunction {
                        exported: *export,
                        name: (*name_span).into(),
                        body: self.function(body),
                    },
                },
            ),
        };
        EclStmt {
            kind,
            span: statement.span().into(),
            data,
        }
    }

    fn annotation(&self, annotation: &ast::TypeAnnotation<'_>) -> EclType {
        ensure_stack(|| self.annotation_node(annotation))
    }

    fn annotation_node(&self, annotation: &ast::TypeAnnotation<'_>) -> EclType {
        use ast::Spanned;
        use ast::TypeAnnotation as T;

        let inner = |inner| EclTypeData {
            inner: self.type_ptr(inner),
        };
        let list = |types| EclTypeData {
            list: self.types(types),
        };
        let (kind, data) = match annotation {
            T::Named { .. } => (EclTypeKind::Named, EclTypeData { none: 0 }),
            T::Array { element, .. } => (EclTypeKind::Array, inner(element)),
            T::Optional { inner: of, .. } => (EclTypeKind::Optional, inner(of)),
            T::Variadic { inner: of, .. } => (EclTypeKind::Variadic, inner(of)),
            T::Map { key, value, .. } => (
                EclTypeKind::Map,
                EclTypeData {
                    map: EclMapType {
                        key: self.type_ptr(key),
                        value: self.type_ptr(value),
                    },
                },
            ),
            T::Tuple { elements, .. } => (EclTypeKind::Tuple, list(elements)),
            T::Union { variants, .. } => (EclTypeKind::Union, list(variants)),
            T::Intersection { parts, .. } => (EclTypeKind::Intersection, list(parts)),
            T::Function {
                type_parameters,
                parameters,
                returns,
                ..
            } => (
                EclTypeKind::Function,
                EclTypeData {
                    function: EclFunctionType {
                        type_parameters: self.type_parameters(type_parameters),
                        parameters: self.list(parameters, |p| EclFunctionTypeParameter {
                            name: p.name.map_or(EclRange::default(), |n| n.span.into()),
                            type_: self.type_ptr(&p.type_annotation),
                        }),
                        returns: self.types(returns),
                    },
                },
            ),
            T::Generic {
                name_span,
                arguments,
                ..
            } => (
                EclTypeKind::Generic,
                EclTypeData {
                    generic: EclGenericType {
                        module: EclRange::default(),
                        name: (*name_span).into(),
                        has_arguments: true,
                        arguments: self.types(arguments),
                    },
                },
            ),
            T::Qualified {
                module,
                name,
                arguments,
                ..
            } => (
                EclTypeKind::Qualified,
                EclTypeData {
                    generic: EclGenericType {
                        module: module.span.into(),
                        name: name.span.into(),
                        has_arguments: arguments.is_some(),
                        arguments: arguments.map_or(EclList::EMPTY, |a| self.types(a)),
                    },
                },
            ),
            T::Typeof { expression, .. } => (
                EclTypeKind::Typeof,
                EclTypeData {
                    typeof_: self.expr_ptr(expression),
                },
            ),
            T::Singleton { value, span } => (
                EclTypeKind::Singleton,
                EclTypeData {
                    singleton: match value {
                        ast::SingletonValue::String(_) => EclSingleton {
                            is_string: true,
                            boolean: false,
                            string: self.bytes(&string::decode_literal(self.text(*span))),
                        },
                        ast::SingletonValue::Boolean(value) => EclSingleton {
                            is_string: false,
                            boolean: *value,
                            string: EclBytes::EMPTY,
                        },
                    },
                },
            ),
            T::Pack { span, .. } => {
                let name = Span::new(span.start, span.end.saturating_sub(3));
                (EclTypeKind::Pack, EclTypeData { name: name.into() })
            }
            T::Struct { fields, .. } => (
                EclTypeKind::Struct,
                EclTypeData {
                    struct_: self.list(fields, |field| {
                        let (name, indexer) = match &field.key {
                            ast::TableTypeKey::Name(name) => (name.span.into(), ptr::null()),
                            ast::TableTypeKey::Indexer(key) => {
                                (EclRange::default(), self.type_ptr(key))
                            }
                        };
                        EclTableTypeField {
                            access: match field.access {
                                None => EclTableTypeAccess::None,
                                Some(ast::TableAccess::Read) => EclTableTypeAccess::Read,
                                Some(ast::TableAccess::Write) => EclTableTypeAccess::Write,
                            },
                            name,
                            indexer,
                            value: self.type_ptr(&field.value),
                        }
                    }),
                },
            ),
        };
        EclType {
            kind,
            span: annotation.span().into(),
            data,
        }
    }
}

fn unary_op(operator: ast::UnaryOperator) -> EclUnaryOp {
    use ast::UnaryOperator as U;
    match operator {
        U::Negate => EclUnaryOp::Negate,
        U::Not => EclUnaryOp::Not,
        U::Length => EclUnaryOp::Length,
        U::BitwiseNot => EclUnaryOp::BitwiseNot,
    }
}

fn binary_op(operator: ast::BinaryOperator) -> EclBinaryOp {
    use ast::BinaryOperator as B;
    match operator {
        B::Add => EclBinaryOp::Add,
        B::Subtract => EclBinaryOp::Subtract,
        B::Multiply => EclBinaryOp::Multiply,
        B::Divide => EclBinaryOp::Divide,
        B::FloorDivide => EclBinaryOp::FloorDivide,
        B::Modulo => EclBinaryOp::Modulo,
        B::Power => EclBinaryOp::Power,
        B::Concatenate => EclBinaryOp::Concatenate,
        B::BitwiseAnd => EclBinaryOp::BitwiseAnd,
        B::BitwiseOr => EclBinaryOp::BitwiseOr,
        B::BitwiseXor => EclBinaryOp::BitwiseXor,
        B::ShiftLeft => EclBinaryOp::ShiftLeft,
        B::ShiftRight => EclBinaryOp::ShiftRight,
        B::Equal => EclBinaryOp::Equal,
        B::NotEqual => EclBinaryOp::NotEqual,
        B::LessThan => EclBinaryOp::LessThan,
        B::GreaterThan => EclBinaryOp::GreaterThan,
        B::LessEqual => EclBinaryOp::LessEqual,
        B::GreaterEqual => EclBinaryOp::GreaterEqual,
        B::And => EclBinaryOp::And,
        B::Or => EclBinaryOp::Or,
    }
}

fn compound_op(operator: ast::CompoundOperator) -> EclCompoundOp {
    use ast::CompoundOperator as C;
    match operator {
        C::Add => EclCompoundOp::Add,
        C::Subtract => EclCompoundOp::Subtract,
        C::Multiply => EclCompoundOp::Multiply,
        C::Divide => EclCompoundOp::Divide,
        C::FloorDivide => EclCompoundOp::FloorDivide,
        C::Modulo => EclCompoundOp::Modulo,
        C::Power => EclCompoundOp::Power,
        C::Concatenate => EclCompoundOp::Concatenate,
        C::BitwiseAnd => EclCompoundOp::BitwiseAnd,
        C::BitwiseOr => EclCompoundOp::BitwiseOr,
        C::ShiftLeft => EclCompoundOp::ShiftLeft,
        C::ShiftRight => EclCompoundOp::ShiftRight,
    }
}
