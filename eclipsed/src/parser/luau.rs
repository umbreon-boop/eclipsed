//! Luau statements and expressions: attributes, `continue`, `const`, type
//! aliases, type functions, `if` expressions and string interpolation.

use crate::ast::*;
use crate::lexer::{Lexer, Span, Token, Type};
use crate::parser::{ParseError, Parser};

impl<'source, 'arena, 'interner> Parser<'source, 'arena, 'interner> {
    /// Parses an optional `local`/`const` binding and its initializer at an
    /// if-condition boundary.
    pub(crate) fn parse_luau_if_condition(
        &mut self,
    ) -> Result<(Option<IfConditionBinding<'arena>>, Expression<'arena>), ParseError> {
        if !self.dialect.has_if_expression() {
            return Ok((None, self.parse_expression()?));
        }

        let is_const = match self.peek_type() {
            Some(Type::Local) => false,
            _ if self.peek_is_word_at(0, "const")
                && self.peek_type_at(1) == Some(Type::Identifier) =>
            {
                true
            }
            _ => return Ok((None, self.parse_expression()?)),
        };

        self.position += 1;
        let name = self.expect(Type::Identifier, "variable name after if binding")?;
        let type_annotation = if self.consume(Type::Colon) {
            Some(self.parse_type_annotation()?)
        } else {
            None
        };
        self.expect(Type::Assign, "'=' after if binding")?;
        let value = self.parse_expression()?;
        let binding = IfConditionBinding {
            variable: LocalVariable {
                symbol: self.intern_token(name),
                name_span: name.span,
                type_annotation,
                attribute: None,
            },
            is_const,
        };
        Ok((Some(binding), value))
    }

    /// Parses a statement introduced by Luau-only syntax, if `token` opens one.
    pub(crate) fn parse_luau_statement(
        &mut self,
        token: &Token,
    ) -> Result<Option<Statement<'arena>>, ParseError> {
        let start = token.span.start;
        if self.dialect.has_attributes() && token.token_type == Type::At {
            return self.parse_attributed_function(start).map(Some);
        }
        if !self.dialect.keywords_are_contextual() || token.token_type != Type::Identifier {
            return Ok(None);
        }
        let next = self.peek_type_at(1);
        let statement = match self.token_str(token) {
            "continue" if self.dialect.has_continue() && !continues_expression(next) => {
                self.position += 1;
                Statement::Continue { span: token.span }
            }
            "type" if next == Some(Type::Function) => self.parse_type_function(false, start)?,
            "type"
                if next == Some(Type::Identifier)
                    && matches!(self.peek_type_at(2), Some(Type::LessThan | Type::Assign)) =>
            {
                self.parse_type_alias(false, start)?
            }
            "export" if self.peek_is_word_at(1, "type") => match self.peek_type_at(2) {
                Some(Type::Function) => self.parse_type_function(true, start)?,
                Some(Type::Identifier) => self.parse_type_alias(true, start)?,
                _ => return Ok(None),
            },
            "const"
                if self.dialect.has_const_declarations()
                    && matches!(next, Some(Type::Identifier | Type::Function)) =>
            {
                self.parse_const(start)?
            }
            _ => return Ok(None),
        };
        Ok(Some(statement))
    }

    fn peek_is_word_at(&self, offset: usize, word: &str) -> bool {
        self.tokens
            .get(self.position + offset)
            .is_some_and(|t| t.token_type == Type::Identifier && self.token_str(t) == word)
    }

    pub(crate) fn parse_attributes(&mut self) -> Result<&'arena [SpannedSymbol], ParseError> {
        let mut attributes = Vec::new();
        while self.consume(Type::At) {
            let name = self.expect(Type::Identifier, "attribute name")?;
            attributes.push(self.spanned_symbol(name));
        }
        Ok(self.alloc_list(attributes))
    }

    fn parse_attributed_function(&mut self, start: u32) -> Result<Statement<'arena>, ParseError> {
        let attributes = self.parse_attributes()?;
        self.pending_attributes = Some(attributes);

        let statement = match (self.peek_type(), self.peek_type_at(1)) {
            (Some(Type::Function), _) => self.parse_function(start)?,
            (Some(Type::Local), Some(Type::Function)) => self.parse_local(start)?,
            _ => return Err(self.expected("function declaration after attributes")),
        };
        debug_assert!(self.pending_attributes.is_none());
        Ok(statement)
    }

    /// `const` requires an initializer for every name; a trailing call or
    /// `...` may still fan out over several.
    fn parse_const(&mut self, start: u32) -> Result<Statement<'arena>, ParseError> {
        self.position += 1;

        if self.consume(Type::Function) {
            let name = self.expect(Type::Identifier, "function name")?;
            let variable = LocalVariable {
                symbol: self.intern_token(name),
                name_span: name.span,
                type_annotation: None,
                attribute: None,
            };
            let body = self.parse_function_body()?;
            let span = Span::new(start, body.span.end);
            let closure = Expression::Closure {
                span: body.span,
                body: self.arena.alloc(body),
            };
            return Ok(Statement::Const {
                variables: std::slice::from_ref(self.arena.alloc(variable)),
                values: std::slice::from_ref(self.arena.alloc(closure)),
                span,
            });
        }

        let variables = self.parse_local_varlist()?;
        self.expect(Type::Assign, "'=' (const declarations require a value)")?;
        let values = self.parse_expression_list()?;
        let span = Span::new(start, self.last_span_end(start));

        let fans_out = matches!(
            values.last(),
            Some(
                Expression::Call { .. } | Expression::MethodCall { .. } | Expression::Vararg { .. }
            )
        );
        let expected = if values.len() > variables.len() {
            "as many const variables as values"
        } else if values.len() < variables.len() && !fans_out {
            "a value for every const variable"
        } else {
            return Ok(Statement::Const {
                variables,
                values,
                span,
            });
        };
        Err(ParseError::Expected {
            expected,
            got: format!("{} variables, {} values", variables.len(), values.len()),
            span,
        })
    }

    /// `if c then a elseif d then b else e`, at the `if`.
    pub(crate) fn parse_if_expression(&mut self) -> Result<Expression<'arena>, ParseError> {
        let start = self.advance()?.span.start;
        let (binding, condition) = self.parse_luau_if_condition()?;
        self.expect(Type::Then, "'then'")?;
        let then_branch = self.parse_expression()?;

        let mut elseifs = Vec::new();
        while self.consume(Type::Elseif) {
            let (binding, condition) = self.parse_luau_if_condition()?;
            self.expect(Type::Then, "'then'")?;
            let result = self.parse_expression()?;
            elseifs.push(ElseIfExpression {
                binding,
                condition,
                result,
            });
        }

        self.expect(Type::Else, "'else'")?;
        let else_branch = self.parse_expression()?;
        Ok(Expression::IfElse {
            binding,
            span: Span::new(start, else_branch.span().end),
            condition: self.arena.alloc(condition),
            then_branch: self.arena.alloc(then_branch),
            elseifs: self.alloc_list(elseifs),
            else_branch: self.arena.alloc(else_branch),
        })
    }

    /// Splits an interpolated string token into literal chunks and `{expr}`
    /// holes, parsing each hole over its own range of the source.
    pub(crate) fn parse_interpolated_string(
        &mut self,
        token: &Token,
    ) -> Result<Expression<'arena>, ParseError> {
        self.position += 1;
        let bytes = self.source.as_bytes();
        let inner_end = token.span.end - 1;

        let mut parts = Vec::new();
        let mut chunk_start = token.span.start + 1;
        let mut i = chunk_start;
        while i < inner_end {
            match bytes[i as usize] {
                b'\\' => i = escape_end(bytes, i, inner_end),
                b'{' => {
                    if i > chunk_start {
                        parts.push(InterpolationPart::Literal {
                            value: self.intern_range(chunk_start, i),
                            span: Span::new(chunk_start, i),
                        });
                    }
                    let hole_start = i + 1;
                    if bytes.get(hole_start as usize) == Some(&b'{') {
                        return Err(ParseError::Unexpected {
                            token: "'{{' in interpolated string".to_owned(),
                            span: Span::new(i, hole_start + 1),
                        });
                    }
                    let (tokens, hole_end) = Lexer::lex_hole(
                        self.source,
                        self.arena,
                        self.dialect,
                        hole_start,
                        inner_end,
                    );
                    let expression = self.parse_hole(tokens, hole_end)?;
                    parts.push(InterpolationPart::Expression(expression));
                    i = hole_end + 1;
                    chunk_start = i;
                }
                _ => i += 1,
            }
        }
        if inner_end > chunk_start {
            parts.push(InterpolationPart::Literal {
                value: self.intern_range(chunk_start, inner_end),
                span: Span::new(chunk_start, inner_end),
            });
        }

        Ok(Expression::InterpolatedString {
            parts: self.alloc_list(parts),
            span: token.span,
        })
    }

    fn parse_hole(
        &mut self,
        tokens: &'arena [Token],
        end: u32,
    ) -> Result<Expression<'arena>, ParseError> {
        let mut sub = self.nested_parser(tokens, end);
        let result = sub.parse_expression().and_then(|expression| {
            if sub.is_at_end() {
                Ok(expression)
            } else {
                Err(sub.unexpected())
            }
        });
        self.failed_instantiations = std::mem::take(&mut sub.failed_instantiations);
        result
    }

    /// `[export] type Name<T> = T`, at `export` or `type`.
    fn parse_type_alias(
        &mut self,
        export: bool,
        start: u32,
    ) -> Result<Statement<'arena>, ParseError> {
        self.position += if export { 2 } else { 1 };

        let name = self.expect(Type::Identifier, "type name")?;
        let type_parameters = if self.consume(Type::LessThan) {
            self.parse_type_parameter_list()?
        } else {
            &[]
        };
        self.expect(Type::Assign, "'='")?;
        let value = self.parse_type_annotation()?;
        // Grouping parentheses are not part of `value`'s span.
        let span = Span::new(start, self.last_span_end(start));

        Ok(Statement::TypeAlias {
            export,
            symbol: self.intern_token(name),
            name_span: name.span,
            type_parameters,
            value,
            span,
        })
    }

    /// `[export] type function name(...) ... end`, at `export` or `type`.
    fn parse_type_function(
        &mut self,
        export: bool,
        start: u32,
    ) -> Result<Statement<'arena>, ParseError> {
        self.position += if export { 3 } else { 2 };

        let name = self.expect(Type::Identifier, "type function name")?;
        let body = self.parse_function_body()?;
        Ok(Statement::TypeFunction {
            export,
            symbol: self.intern_token(name),
            name_span: name.span,
            span: Span::new(start, body.span.end),
            body: self.arena.alloc(body),
        })
    }

    fn intern_range(&mut self, start: u32, end: u32) -> crate::interner::Symbol {
        let text = &self.source[start as usize..end as usize];
        self.interner.intern(text)
    }
}

/// Whether a token after `continue` makes it an identifier expression, as in
/// `continue()` or `continue = 1`.
fn continues_expression(next: Option<Type>) -> bool {
    matches!(
        next,
        Some(
            Type::Assign
                | Type::Comma
                | Type::LeftParenthesis
                | Type::LeftSquareBracket
                | Type::LeftCurlyBrace
                | Type::Dot
                | Type::Colon
                | Type::String
                | Type::LongString
                | Type::InterpolatedString
                | Type::PlusAssign
                | Type::MinusAssign
                | Type::StarAssign
                | Type::SlashAssign
                | Type::DoubleSlashAssign
                | Type::PercentAssign
                | Type::CaretAssign
                | Type::DoubleDotAssign
        )
    )
}

/// The offset just past the escape sequence whose `\` is at `i`.
fn escape_end(bytes: &[u8], i: u32, limit: u32) -> u32 {
    let mut end = i + 2;
    if bytes.get(i as usize + 1) == Some(&b'u') && bytes.get(end as usize) == Some(&b'{') {
        end += 1;
        while end < limit && bytes[end as usize].is_ascii_hexdigit() {
            end += 1;
        }
        if end < limit && bytes[end as usize] == b'}' {
            end += 1;
        }
    }
    end
}
