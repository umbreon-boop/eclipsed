//! Byte ranges the parser recognised as Luau type syntax.
//!
//! Only outermost ranges are reported, so each begins and ends on a token
//! boundary; a nested generic's closing `>` may be half of a `>>` token.
//! Whether surrounding punctuation such as the introducing `:` goes with a
//! range is left to the caller.

use crate::api::Chunk;
use crate::ast::*;
use crate::cst::TokenRef;
use crate::lexer::{Span, Type};

/// What kind of type syntax a [`TypeSpan`] covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeSpanKind {
    /// A type annotation, excluding the introducing `:` or `::` but including
    /// any parentheses around it, which the AST does not record.
    Annotation,
    /// A whole `[export] type ...` statement.
    AliasStatement,
    /// The name of a declared generic type parameter.
    Parameter,
}

/// One occurrence of type syntax, as a byte range into the chunk's source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeSpan {
    pub kind: TypeSpanKind,
    pub span: Span,
}

impl Chunk<'_> {
    /// Every occurrence of Luau type syntax in this chunk, sorted by start.
    pub fn type_spans(&self) -> Vec<TypeSpan> {
        let mut collector = Collector {
            tokens: self.tokens(),
            out: Vec::new(),
        };
        collector.block(self.statements());
        collector
            .out
            .sort_by_key(|entry| (entry.span.start, entry.span.end));
        collector.out.dedup();
        collector.out
    }
}

struct Collector<'a> {
    tokens: &'a [TokenRef<'a>],
    out: Vec<TypeSpan>,
}

impl Collector<'_> {
    /// The index of the token starting exactly at `offset`.
    fn at_start(&self, offset: u32) -> Option<usize> {
        let index = self
            .tokens
            .partition_point(|token| token.span.start < offset);
        self.tokens
            .get(index)
            .filter(|token| token.span.start == offset)
            .map(|_| index)
    }

    /// The index of the token ending exactly at `offset`.
    fn at_end(&self, offset: u32) -> Option<usize> {
        let index = self.tokens.partition_point(|token| token.span.end < offset);
        self.tokens
            .get(index)
            .filter(|token| token.span.end == offset)
            .map(|_| index)
    }

    fn kind_at(&self, index: usize) -> Option<Type> {
        self.tokens.get(index).map(|token| token.token_type)
    }

    /// Grows `span` over any `(`/`)` pairs wrapping it exactly.
    fn widen(&self, mut span: Span) -> Span {
        loop {
            let (Some(first), Some(last)) = (self.at_start(span.start), self.at_end(span.end))
            else {
                return span;
            };
            if first == 0 {
                return span;
            }
            if self.kind_at(first - 1) != Some(Type::LeftParenthesis)
                || self.kind_at(last + 1) != Some(Type::RightParenthesis)
            {
                return span;
            }
            span = Span::new(
                self.tokens[first - 1].span.start,
                self.tokens[last + 1].span.end,
            );
        }
    }

    fn annotation(&mut self, annotation: &TypeAnnotation<'_>) {
        self.push_annotation(annotation.span());
    }

    fn opt_annotation(&mut self, annotation: Option<&TypeAnnotation<'_>>) {
        if let Some(annotation) = annotation {
            self.annotation(annotation);
        }
    }

    fn binding(&mut self, binding: Option<&IfConditionBinding<'_>>) {
        self.opt_annotation(binding.and_then(|binding| binding.variable.type_annotation.as_ref()));
    }

    /// One span covering a delimited list such as a return list, so the
    /// separators between the elements go with it.
    fn annotation_list(&mut self, list: &[TypeAnnotation<'_>]) {
        if let (Some(first), Some(last)) = (list.first(), list.last()) {
            self.push_annotation(Span::new(first.span().start, last.span().end));
        }
    }

    fn push_annotation(&mut self, span: Span) {
        let span = self.widen(span);
        self.out.push(TypeSpan {
            kind: TypeSpanKind::Annotation,
            span,
        });
    }

    /// The index one past the balanced group starting at `index`. `>>` counts
    /// as two closers.
    fn skip_group(&self, index: usize, opens: Type, closes: Type) -> Option<usize> {
        if self.kind_at(index) != Some(opens) {
            return None;
        }
        let mut depth = 0i32;
        let mut index = index;
        loop {
            let kind = self.kind_at(index)?;
            if kind == opens {
                depth += 1;
            } else if kind == closes {
                depth -= 1;
            } else if closes == Type::GreaterThan && kind == Type::ShiftRight {
                depth -= 2;
            } else if opens == Type::LessThan && kind == Type::ShiftLeft {
                depth += 2;
            }
            index += 1;
            match depth.cmp(&0) {
                std::cmp::Ordering::Equal => return Some(index),
                // Half of a `>>` shared with an enclosing group.
                std::cmp::Ordering::Less => return None,
                std::cmp::Ordering::Greater => {}
            }
        }
    }

    /// The `()` of `function f(): ()`, which has no annotation to point at.
    fn empty_returns(&mut self, body: &FunctionBody<'_>) {
        let Some(mut index) = self.at_start(body.span.start) else {
            return;
        };
        if self.kind_at(index) == Some(Type::LessThan) {
            let Some(next) = self.skip_group(index, Type::LessThan, Type::GreaterThan) else {
                return;
            };
            index = next;
        }
        let Some(next) = self.skip_group(index, Type::LeftParenthesis, Type::RightParenthesis)
        else {
            return;
        };
        index = next;

        let (Some(Type::Colon), Some(Type::LeftParenthesis), Some(Type::RightParenthesis)) = (
            self.kind_at(index),
            self.kind_at(index + 1),
            self.kind_at(index + 2),
        ) else {
            return;
        };
        self.out.push(TypeSpan {
            kind: TypeSpanKind::Annotation,
            span: Span::new(
                self.tokens[index + 1].span.start,
                self.tokens[index + 2].span.end,
            ),
        });
    }

    fn type_parameters(&mut self, parameters: &[TypeParameter<'_>]) {
        for parameter in parameters {
            self.out.push(TypeSpan {
                kind: TypeSpanKind::Parameter,
                span: parameter.name_span,
            });
        }
    }

    fn body(&mut self, body: &FunctionBody<'_>) {
        self.type_parameters(body.type_parameters);
        for parameter in body.parameters {
            self.opt_annotation(parameter.type_annotation.as_ref());
        }
        self.opt_annotation(body.vararg_type.as_ref());
        match body.returns {
            Some([]) => self.empty_returns(body),
            Some(list) => self.annotation_list(list),
            None => {}
        }
        self.block(body.body);
    }

    fn locals(&mut self, variables: &[LocalVariable<'_>]) {
        for variable in variables {
            self.opt_annotation(variable.type_annotation.as_ref());
        }
    }

    fn block(&mut self, statements: &[Statement<'_>]) {
        for statement in statements {
            self.statement(statement);
        }
    }

    fn statement(&mut self, statement: &Statement<'_>) {
        crate::parser::ensure_stack(|| self.statement_node(statement))
    }

    fn statement_node(&mut self, statement: &Statement<'_>) {
        match statement {
            Statement::Local {
                variables, values, ..
            }
            | Statement::Const {
                variables, values, ..
            } => {
                self.locals(variables);
                self.expressions(values);
            }
            Statement::LocalFunction { body, .. } | Statement::Function { body, .. } => {
                self.body(body)
            }
            Statement::Assign {
                targets, values, ..
            } => {
                self.expressions(targets);
                self.expressions(values);
            }
            Statement::Do { body, .. } => self.block(body),
            Statement::If {
                binding,
                condition,
                then_block,
                elseifs,
                else_block,
                ..
            } => {
                self.binding(binding.as_ref());
                self.expression(condition);
                self.block(then_block);
                for arm in *elseifs {
                    self.binding(arm.binding.as_ref());
                    self.expression(&arm.condition);
                    self.block(arm.body);
                }
                if let Some(block) = else_block {
                    self.block(block);
                }
            }
            Statement::While {
                condition, body, ..
            } => {
                self.expression(condition);
                self.block(body);
            }
            Statement::Repeat { body, until, .. } => {
                self.block(body);
                self.expression(until);
            }
            Statement::NumericFor {
                variable_type,
                start,
                limit,
                step,
                body,
                ..
            } => {
                self.opt_annotation(variable_type.as_ref());
                self.expression(start);
                self.expression(limit);
                if let Some(step) = step {
                    self.expression(step);
                }
                self.block(body);
            }
            Statement::GenericFor {
                variables,
                iterators,
                body,
                ..
            } => {
                for variable in *variables {
                    self.opt_annotation(variable.type_annotation.as_ref());
                }
                self.expressions(iterators);
                self.block(body);
            }
            Statement::Return { values, .. } => self.expressions(values),
            Statement::Expression { expr, .. } => self.expression(expr),
            Statement::Break { .. }
            | Statement::Continue { .. }
            | Statement::Goto { .. }
            | Statement::Label { .. } => {}
            #[cfg(feature = "compound")]
            Statement::CompoundAssign { target, value, .. } => {
                self.expression(target);
                self.expression(value);
            }
            Statement::TypeAlias { span, .. } | Statement::TypeFunction { span, .. } => {
                self.out.push(TypeSpan {
                    kind: TypeSpanKind::AliasStatement,
                    span: *span,
                })
            }
            #[cfg(feature = "cfxlua")]
            Statement::InUnpack {
                variables, source, ..
            } => {
                self.locals(variables);
                self.expression(source);
            }
        }
    }

    fn expressions(&mut self, expressions: &[Expression<'_>]) {
        for expression in expressions {
            self.expression(expression);
        }
    }

    fn expression(&mut self, expression: &Expression<'_>) {
        crate::parser::ensure_stack(|| self.expression_node(expression))
    }

    fn expression_node(&mut self, expression: &Expression<'_>) {
        match expression {
            Expression::Unary { operand, .. } => self.expression(operand),
            Expression::Binary { left, right, .. } => {
                self.expression(left);
                self.expression(right);
            }
            Expression::Table { fields, .. } => self.table(fields),
            Expression::Closure { body, .. } => self.body(body),
            Expression::Call {
                callee, arguments, ..
            } => {
                self.expression(callee);
                self.arguments(arguments);
            }
            Expression::MethodCall {
                receiver,
                arguments,
                ..
            } => {
                self.expression(receiver);
                self.arguments(arguments);
            }
            Expression::Field { table, .. } => self.expression(table),
            Expression::Index { table, key, .. } => {
                self.expression(table);
                self.expression(key);
            }
            Expression::Cast {
                expression, target, ..
            } => {
                self.expression(expression);
                self.annotation(target);
            }
            Expression::Paren { expression, .. } => self.expression(expression),
            Expression::IfElse {
                binding,
                condition,
                then_branch,
                elseifs,
                else_branch,
                ..
            } => {
                self.binding(binding.as_ref());
                self.expression(condition);
                self.expression(then_branch);
                for arm in *elseifs {
                    self.binding(arm.binding.as_ref());
                    self.expression(&arm.condition);
                    self.expression(&arm.result);
                }
                self.expression(else_branch);
            }
            Expression::InterpolatedString { parts, .. } => {
                for part in *parts {
                    if let InterpolationPart::Expression(expression) = part {
                        self.expression(expression);
                    }
                }
            }
            _ => {}
        }
    }

    fn table(&mut self, fields: &[TableField<'_>]) {
        for field in fields {
            if let TableKey::Expression(key) = &field.key {
                self.expression(key);
            }
            self.expression(&field.value);
        }
    }

    fn arguments(&mut self, arguments: &CallArguments<'_>) {
        match arguments {
            CallArguments::Expressions {
                type_arguments,
                arguments,
            } => {
                if let Some(list) = type_arguments {
                    self.annotation_list(list);
                }
                self.expressions(arguments);
            }
            CallArguments::Table { fields, .. } => self.table(fields),
            CallArguments::String { .. } => {}
        }
    }
}
