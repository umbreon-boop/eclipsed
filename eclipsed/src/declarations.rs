//! Module-level declarations, the members a documentation tool lists.
//!
//! Top-level statements are searched, along with `do` and `if` blocks, which
//! may define members conditionally; function bodies and loops are not.

use crate::api::Chunk;
use crate::ast::{
    Expression, FunctionBody, LocalVariable, Spanned, Statement, TableField, TableKey,
};
use crate::lexer::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeclarationKind {
    Function,
    Field,
    #[cfg(feature = "luau")]
    Type,
    #[cfg(feature = "luau")]
    TypeFunction,
}

/// How a name segment joins the one before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NameJoin {
    /// `.`, or nothing for the first segment.
    Dot,
    /// `:`
    Method,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub kind: DeclarationKind,
    /// Whether the name is a `local` rather than a global or a field.
    pub local: bool,
    /// The name, one segment per span. The segments of `local M = { f = … }`
    /// are not adjacent in the source.
    pub name: Vec<(NameJoin, Span)>,
    pub parameters: Vec<Span>,
    pub vararg: bool,
    /// Leading comments of the declaration's first token, verbatim.
    pub doc: Vec<Span>,
    /// Where the declaration starts; its doc comments lead this token.
    pub span: Span,
}

impl Chunk<'_> {
    pub fn declarations(&self) -> Vec<Declaration> {
        let mut collector = Collector {
            chunk: self,
            out: Vec::new(),
        };
        collector.block(self.statements());
        collector.out
    }
}

type Name = Vec<(NameJoin, Span)>;

struct Collector<'c, 'a> {
    chunk: &'c Chunk<'a>,
    out: Vec<Declaration>,
}

impl Collector<'_, '_> {
    fn declare(
        &mut self,
        kind: DeclarationKind,
        local: bool,
        name: Name,
        body: Option<&FunctionBody<'_>>,
        span: Span,
    ) {
        self.out.push(Declaration {
            kind,
            local,
            name,
            parameters: body.map_or_else(Vec::new, |body| {
                body.parameters.iter().map(|p| p.name_span).collect()
            }),
            vararg: body.is_some_and(|body| body.has_vararg),
            doc: self.chunk.doc_comment_spans_for(span),
            span,
        });
    }

    fn value(&mut self, local: bool, name: Name, value: &Expression<'_>, span: Span) {
        match value {
            Expression::Closure { body, .. } => {
                self.declare(DeclarationKind::Function, local, name, Some(body), span)
            }
            _ => self.declare(DeclarationKind::Field, local, name, None, span),
        }
    }

    fn table(&mut self, prefix: Option<Span>, fields: &[TableField<'_>]) {
        for field in fields {
            let TableKey::Name { span, .. } = field.key else {
                continue;
            };
            let mut name: Name = prefix.map(|p| (NameJoin::Dot, p)).into_iter().collect();
            name.push((NameJoin::Dot, span));
            self.value(false, name, &field.value, span);
        }
    }

    fn local(&mut self, variables: &[LocalVariable<'_>], values: &[Expression<'_>], span: Span) {
        for (variable, value) in variables.iter().zip(values) {
            match value {
                Expression::Table { fields, .. } => self.table(Some(variable.name_span), fields),
                Expression::Closure { body, .. } => self.declare(
                    DeclarationKind::Function,
                    true,
                    vec![(NameJoin::Dot, variable.name_span)],
                    Some(body),
                    span,
                ),
                _ => {}
            }
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
            Statement::Function { name, body, span } => {
                let mut path: Name = name
                    .path
                    .iter()
                    .map(|part| (NameJoin::Dot, part.span))
                    .collect();
                if let Some(method) = name.method {
                    path.push((NameJoin::Method, method.span));
                }
                self.declare(DeclarationKind::Function, false, path, Some(body), *span);
            }
            Statement::LocalFunction {
                name_span,
                body,
                span,
                ..
            } => self.declare(
                DeclarationKind::Function,
                true,
                vec![(NameJoin::Dot, *name_span)],
                Some(body),
                *span,
            ),
            #[cfg(feature = "luau")]
            Statement::TypeAlias {
                name_span, span, ..
            } => self.declare(
                DeclarationKind::Type,
                false,
                vec![(NameJoin::Dot, *name_span)],
                None,
                *span,
            ),
            #[cfg(feature = "luau")]
            Statement::TypeFunction {
                name_span,
                body,
                span,
                ..
            } => self.declare(
                DeclarationKind::TypeFunction,
                false,
                vec![(NameJoin::Dot, *name_span)],
                Some(body),
                *span,
            ),
            Statement::Assign {
                targets, values, ..
            } => {
                for (target, value) in targets.iter().zip(values.iter()) {
                    if let Some(name) = target_name(target) {
                        self.value(false, name, value, target.span());
                    }
                }
            }
            Statement::Local {
                variables,
                values,
                span,
            } => self.local(variables, values, *span),
            #[cfg(feature = "luau")]
            Statement::Const {
                variables,
                values,
                span,
            } => self.local(variables, values, *span),
            Statement::Return { values, .. } => {
                if let Some(Expression::Table { fields, .. }) = values.first() {
                    self.table(None, fields);
                }
            }
            Statement::Do { body, .. } => self.block(body),
            Statement::If {
                then_block,
                elseifs,
                else_block,
                ..
            } => {
                self.block(then_block);
                for arm in *elseifs {
                    self.block(arm.body);
                }
                if let Some(block) = else_block {
                    self.block(block);
                }
            }
            _ => {}
        }
    }
}

/// The dotted name an assignment target denotes, like `M.a.b`.
fn target_name(target: &Expression<'_>) -> Option<Name> {
    match target {
        Expression::Identifier { span, .. } => Some(vec![(NameJoin::Dot, *span)]),
        Expression::Field {
            table, field_span, ..
        } => {
            let mut name = target_name(table)?;
            name.push((NameJoin::Dot, *field_span));
            Some(name)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Dialect;
    use crate::api::with_chunk;

    fn names(source: &str) -> Vec<String> {
        names_in(source, Dialect::Lua)
    }

    fn names_in(source: &str, dialect: Dialect) -> Vec<String> {
        with_chunk(source, dialect, |chunk| {
            chunk
                .declarations()
                .iter()
                .map(|decl| {
                    let mut name = String::new();
                    for (index, (join, span)) in decl.name.iter().enumerate() {
                        if index > 0 {
                            name.push(if *join == NameJoin::Method { ':' } else { '.' });
                        }
                        name.push_str(chunk.text(*span));
                    }
                    name
                })
                .collect()
        })
        .unwrap()
    }

    #[test]
    fn common_forms() {
        let source = "\
function M.a() end
function M:b() end
local function c() end
local d = function() end
M.e, M.f = 1, function() end
if x then elseif y then function M.g() end else function M.h() end end
local N = { i = 1 }
return { j = function() end }
";
        assert_eq!(
            names(source),
            [
                "M.a", "M:b", "c", "d", "M.e", "M.f", "M.g", "M.h", "N.i", "j"
            ]
        );
    }

    #[test]
    fn function_bodies_are_not_searched() {
        assert_eq!(names("function M.a() function M.inner() end end"), ["M.a"]);
    }

    #[cfg(feature = "luau")]
    #[test]
    fn luau_const_declarations_are_collected() {
        assert_eq!(
            names_in(
                "const f = function() end\nconst M = { g = 1 }",
                Dialect::Luau
            ),
            ["f", "M.g"]
        );
    }
}
