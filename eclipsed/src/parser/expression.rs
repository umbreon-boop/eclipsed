use std::borrow::Cow;

use crate::ast::*;
use crate::lexer::*;
use crate::parser::{ParseError, Parser};

impl<'source, 'arena, 'interner> Parser<'source, 'arena, 'interner> {
    pub(crate) fn parse_expression(&mut self) -> Result<Expression<'arena>, ParseError> {
        self.parse_operand_precedence(0, false)
    }

    /// Parses the expression opening a statement; see
    /// [`Parser::stops_at_ambiguous_call`].
    pub(crate) fn parse_statement_expression(&mut self) -> Result<Expression<'arena>, ParseError> {
        self.parse_operand_precedence(0, true)
    }

    pub(crate) fn parse_expression_list(
        &mut self,
    ) -> Result<&'arena [Expression<'arena>], ParseError> {
        let expressions = self.comma_list(Self::parse_expression)?;
        Ok(self.alloc_list(expressions))
    }

    fn parse_precedence(&mut self, min_bp: u8) -> Result<Expression<'arena>, ParseError> {
        self.parse_operand_precedence(min_bp, false)
    }

    fn parse_operand_precedence(
        &mut self,
        min_bp: u8,
        as_statement: bool,
    ) -> Result<Expression<'arena>, ParseError> {
        self.nested(|this| {
            let mut left = this.parse_prefix(as_statement)?;
            loop {
                let start = left.span().start;

                #[cfg(feature = "luau")]
                if this.dialect.has_type_cast() && this.consume(Type::DoubleColon) {
                    let target = this.parse_type_annotation()?;
                    let span = Span::new(start, this.type_end(&target));
                    left = Expression::Cast {
                        expression: this.arena.alloc(left),
                        target: this.arena.alloc(target),
                        span,
                    };
                    continue;
                }

                let Some((operator, left_bp, right_bp)) =
                    this.peek_operator().and_then(binary_operator)
                else {
                    break;
                };
                if left_bp < min_bp {
                    break;
                }
                this.position += 1;
                let right = this.parse_precedence(right_bp)?;
                let span = Span::new(start, right.span().end);
                left = Expression::Binary {
                    left: this.arena.alloc(left),
                    operator,
                    right: this.arena.alloc(right),
                    span,
                };
            }
            Ok(left)
        })
    }

    /// Parses one operand: a `prefixexp` with its suffix chain, or a literal,
    /// constructor or unary application. Only `prefixexp`s take suffixes, so
    /// `34(s)` and `{}[1]` are not expressions.
    fn parse_prefix(&mut self, as_statement: bool) -> Result<Expression<'arena>, ParseError> {
        let Some(&token) = self.peek() else {
            return Err(self.unexpected());
        };
        let span = token.span;

        if let Some(operator) = unary_operator(token.token_type) {
            self.position += 1;
            let operand = self.parse_precedence(UNARY_BINDING_POWER)?;
            let span = Span::new(span.start, operand.span().end);
            return Ok(Expression::Unary {
                operator,
                operand: self.arena.alloc(operand),
                span,
            });
        }

        let expression = match token.token_type {
            Type::Nil => Expression::Nil { span },
            Type::True => Expression::True { span },
            Type::False => Expression::False { span },
            Type::TripleDot => Expression::Vararg { span },
            Type::Integer | Type::Float => {
                self.position += 1;
                #[cfg(feature = "luajit")]
                if self.dialect.has_cdata_suffixes()
                    && let Some(expr) = self.parse_cdata_literal(&token)
                {
                    return Ok(expr);
                }
                if token.token_type == Type::Integer {
                    return self.parse_integer(&token);
                }
                let value = self.parse_float_value(self.token_str(&token), span)?;
                return Ok(Expression::Float { value, span });
            }
            Type::String | Type::LongString => Expression::String {
                value: self.intern_token(&token),
                span,
            },
            #[cfg(feature = "cfxlua")]
            Type::HashString => Expression::JenkinsHash {
                value: self.intern_token(&token),
                span,
            },
            Type::Identifier => {
                self.position += 1;
                let base = Expression::Identifier {
                    name: self.intern_token(&token),
                    span,
                };
                return self.parse_suffixes(span.start, base, as_statement);
            }
            Type::LeftParenthesis => {
                self.position += 1;
                let inner = self.parse_expression()?;
                let close = self.expect(Type::RightParenthesis, "')'")?;
                let base = Expression::Paren {
                    expression: self.arena.alloc(inner),
                    span: Span::new(span.start, close.span.end),
                };
                return self.parse_suffixes(span.start, base, as_statement);
            }
            Type::Function => {
                self.position += 1;
                return self.parse_closure(span.start);
            }
            Type::LeftCurlyBrace => {
                let (fields, span) = self.parse_table_fields()?;
                return Ok(Expression::Table { fields, span });
            }
            #[cfg(feature = "luau")]
            Type::At if self.dialect.has_attributes() => {
                let attributes = self.parse_attributes()?;
                self.pending_attributes = Some(attributes);
                self.expect(Type::Function, "'function' after attributes")?;
                return self.parse_closure(span.start);
            }
            #[cfg(feature = "luau")]
            Type::If if self.dialect.has_if_expression() => return self.parse_if_expression(),
            #[cfg(feature = "luau")]
            Type::InterpolatedString => return self.parse_interpolated_string(&token),
            _ => return Err(self.unexpected()),
        };
        self.position += 1;
        Ok(expression)
    }

    /// A function expression's body, after `function`.
    fn parse_closure(&mut self, start: u32) -> Result<Expression<'arena>, ParseError> {
        let body = self.parse_function_body()?;
        Ok(Expression::Closure {
            span: Span::new(start, body.span.end),
            body: self.arena.alloc(body),
        })
    }

    /// Radix-prefixed integers wrap modulo 2^64; decimal integers that
    /// overflow `i64` denote floats (Lua 5.4 §3.1).
    fn parse_integer(&mut self, token: &Token) -> Result<Expression<'arena>, ParseError> {
        let span = token.span;
        let text = strip_separators(self.token_str(token));
        let radix = match text.get(..2) {
            Some("0x" | "0X") => Some(16),
            Some("0b" | "0B") => Some(2),
            _ => None,
        };
        if let Some(radix) = radix {
            return match wrapping_radix(&text[2..], radix) {
                Some(value) => Ok(Expression::Integer {
                    value: value as i64,
                    span,
                }),
                None => Err(ParseError::Expected {
                    expected: "valid integer",
                    got: text.into_owned(),
                    span,
                }),
            };
        }
        match text.parse::<i64>() {
            Ok(value) => Ok(Expression::Integer { value, span }),
            Err(_) => Ok(Expression::Float {
                value: self.parse_float_value(&text, span)?,
                span,
            }),
        }
    }

    /// Consumes the suffix chain after `base`: `.name`, `[key]`,
    /// `:method(args)`, call arguments and the dialect-specific suffixes.
    fn parse_suffixes(
        &mut self,
        start: u32,
        mut base: Expression<'arena>,
        as_statement: bool,
    ) -> Result<Expression<'arena>, ParseError> {
        loop {
            match self.peek_type() {
                Some(Type::Dot) => {
                    self.position += 1;
                    let name = self.expect_field_name("field name")?;
                    base = Expression::Field {
                        table: self.arena.alloc(base),
                        field: self.intern_token(name),
                        field_span: name.span,
                        safe: false,
                        span: Span::new(start, name.span.end),
                    };
                }
                Some(Type::LeftSquareBracket) => {
                    self.position += 1;
                    let key = self.parse_expression()?;
                    let close = self.expect(Type::RightSquareBracket, "']'")?;
                    base = Expression::Index {
                        table: self.arena.alloc(base),
                        key: self.arena.alloc(key),
                        span: Span::new(start, close.span.end),
                    };
                }
                Some(Type::Colon) => {
                    self.position += 1;
                    let method = self.expect_field_name("method name")?;

                    #[cfg(feature = "luau")]
                    let type_arguments =
                        if self.dialect.has_type_cast() && self.consume(Type::ShiftLeft) {
                            Some(self.parse_instantiation_type_arguments()?)
                        } else {
                            None
                        };
                    #[cfg(not(feature = "luau"))]
                    let type_arguments = None;

                    let arguments = self.parse_call_arguments(type_arguments)?;
                    base = Expression::MethodCall {
                        receiver: self.arena.alloc(base),
                        method: self.intern_token(method),
                        method_span: method.span,
                        arguments,
                        span: Span::new(start, self.last_span_end(start)),
                    };
                }
                Some(Type::LeftParenthesis)
                    if !as_statement && self.stops_at_ambiguous_call(base.span().end) =>
                {
                    return Ok(base);
                }
                Some(
                    Type::LeftParenthesis | Type::LeftCurlyBrace | Type::String | Type::LongString,
                ) => {
                    let arguments = self.parse_call_arguments(None)?;
                    base = Expression::Call {
                        callee: self.arena.alloc(base),
                        arguments,
                        span: Span::new(start, self.last_span_end(start)),
                    };
                }
                #[cfg(feature = "cfxlua")]
                Some(Type::QuestionMark) if self.dialect.has_safe_navigation() => {
                    self.position += 1;
                    self.expect(Type::Dot, "'.' after '?'")?;
                    let name = self.expect(Type::Identifier, "field name")?;
                    base = Expression::Field {
                        table: self.arena.alloc(base),
                        field: self.intern_token(name),
                        field_span: name.span,
                        safe: true,
                        span: Span::new(start, name.span.end),
                    };
                }
                #[cfg(feature = "luau")]
                Some(Type::ShiftLeft) if self.dialect.has_type_cast() => {
                    match self.try_parse_instantiated_call(start, &base)? {
                        Some(call) => base = call,
                        None => return Ok(base),
                    }
                }
                _ => return Ok(base),
            }
        }
    }

    /// A field or method name. `goto` stays usable here for Lua 5.1 sources.
    pub(crate) fn expect_field_name(
        &mut self,
        expected: &'static str,
    ) -> Result<&'arena Token, ParseError> {
        match self.peek_type() {
            Some(Type::Identifier | Type::Goto) => self.advance(),
            _ => Err(self.expected(expected)),
        }
    }

    /// Whether the `(` at the cursor starts on a later line than the callee
    /// ends, and so, in dialects that split there, ends the suffix chain.
    ///
    /// `as_statement` exempts the statement's own callee, so a deliberate
    /// `f` newline `(x)` call statement still parses as one call.
    fn stops_at_ambiguous_call(&self, callee_end: u32) -> bool {
        if !self.dialect.breaks_ambiguous_call() {
            return false;
        }
        let Some(paren) = self.peek() else {
            return false;
        };
        self.source
            .as_bytes()
            .get(callee_end as usize..paren.span.start as usize)
            .is_some_and(|gap| gap.contains(&b'\n'))
    }

    /// Parses call arguments: `(...)`, or the `f{...}` / `f"s"` sugar.
    fn parse_call_arguments(
        &mut self,
        type_arguments: Option<&'arena [TypeAnnotation<'arena>]>,
    ) -> Result<CallArguments<'arena>, ParseError> {
        match self.peek_type() {
            Some(Type::LeftParenthesis) => {
                self.position += 1;
                let arguments = if self.check(Type::RightParenthesis) {
                    &[]
                } else {
                    self.parse_expression_list()?
                };
                self.expect(Type::RightParenthesis, "')'")?;
                Ok(CallArguments::Expressions {
                    type_arguments,
                    arguments,
                })
            }
            Some(Type::LeftCurlyBrace) => {
                let (fields, span) = self.parse_table_fields()?;
                Ok(CallArguments::Table { fields, span })
            }
            Some(Type::String | Type::LongString) => {
                let token = self.advance()?;
                Ok(CallArguments::String {
                    value: self.intern_token(token),
                    span: token.span,
                })
            }
            _ => Err(self.expected("function arguments")),
        }
    }

    /// The type arguments of `f<<T>>`, after the `<<`, through the `>>`.
    #[cfg(feature = "luau")]
    fn parse_instantiation_type_arguments(
        &mut self,
    ) -> Result<&'arena [TypeAnnotation<'arena>], ParseError> {
        let args = if self.check(Type::ShiftRight) {
            Vec::new()
        } else {
            self.comma_list(Self::parse_type_or_pack)?
        };
        // The rest of a `>>` that closed a nested generic, and a `>` right
        // after it, spell `>>` too: `f<<A<B>>>()`.
        if self.pending_angle == Some(self.position) {
            let half_end = self.tokens[self.position].span.end;
            return match self.tokens.get(self.position + 1) {
                Some(next)
                    if next.token_type == Type::GreaterThan && next.span.start == half_end =>
                {
                    self.pending_angle = None;
                    self.position += 2;
                    Ok(self.alloc_list(args))
                }
                _ => Err(self.expected("'>>'")),
            };
        }
        self.expect(Type::ShiftRight, "'>>'")?;
        Ok(self.alloc_list(args))
    }

    /// Tries `left<<T>>(args)` at a `<<`, rewinding and returning `None` if
    /// the tokens are a shift instead.
    #[cfg(feature = "luau")]
    fn try_parse_instantiated_call(
        &mut self,
        start: u32,
        left: &Expression<'arena>,
    ) -> Result<Option<Expression<'arena>>, ParseError> {
        let offset = self.current_span().start;
        if self.failed_instantiations.contains(&offset) {
            return Ok(None);
        }
        let saved = (self.position, self.pending_angle);
        self.position += 1;
        let type_arguments = match self.parse_instantiation_type_arguments() {
            Ok(arguments) if self.check(Type::LeftParenthesis) => arguments,
            Err(error @ ParseError::TooDeep { .. }) => return Err(error),
            _ => {
                (self.position, self.pending_angle) = saved;
                self.failed_instantiations.insert(offset);
                return Ok(None);
            }
        };
        let arguments = self.parse_call_arguments(Some(type_arguments))?;
        Ok(Some(Expression::Call {
            callee: self.arena.alloc(left.clone()),
            arguments,
            span: Span::new(start, self.last_span_end(start)),
        }))
    }

    pub(crate) fn parse_table_fields(
        &mut self,
    ) -> Result<(&'arena [TableField<'arena>], Span), ParseError> {
        let start = self.expect(Type::LeftCurlyBrace, "'{'")?.span.start;
        let mut fields = Vec::new();
        while !self.check(Type::RightCurlyBrace) && !self.is_at_end() {
            fields.push(self.parse_table_field()?);
            if !self.consume(Type::Comma) && !self.consume(Type::Semicolon) {
                break;
            }
        }
        let end = self.expect(Type::RightCurlyBrace, "'}'")?.span.end;
        Ok((self.alloc_list(fields), Span::new(start, end)))
    }

    fn parse_table_field(&mut self) -> Result<TableField<'arena>, ParseError> {
        // CfxLua `{ .name }` is sugar for `{ name = true }`.
        #[cfg(feature = "cfxlua")]
        if self.dialect.has_set_constructor() && self.consume(Type::Dot) {
            let name = self.expect(Type::Identifier, "set field name")?;
            return Ok(TableField {
                key: TableKey::Name {
                    symbol: self.intern_token(name),
                    span: name.span,
                },
                value: Expression::True { span: name.span },
            });
        }

        if self.consume(Type::LeftSquareBracket) {
            let key = self.parse_expression()?;
            self.expect(Type::RightSquareBracket, "']'")?;
            self.expect(Type::Assign, "'='")?;
            return Ok(TableField {
                key: TableKey::Expression(key),
                value: self.parse_expression()?,
            });
        }

        if self.check(Type::Identifier) && self.peek_type_at(1) == Some(Type::Assign) {
            let name = self.advance()?;
            self.position += 1;
            return Ok(TableField {
                key: TableKey::Name {
                    symbol: self.intern_token(name),
                    span: name.span,
                },
                value: self.parse_expression()?,
            });
        }

        Ok(TableField {
            key: TableKey::None,
            value: self.parse_expression()?,
        })
    }
}

fn strip_separators(text: &str) -> Cow<'_, str> {
    if text.contains('_') {
        Cow::Owned(text.replace('_', ""))
    } else {
        Cow::Borrowed(text)
    }
}

/// Parses `digits` in `radix`, wrapping modulo 2^64.
fn wrapping_radix(digits: &str, radix: u32) -> Option<u64> {
    if digits.is_empty() {
        return None;
    }
    digits.chars().try_fold(0u64, |value, c| {
        let digit = c.to_digit(radix)?;
        Some(
            value
                .wrapping_mul(u64::from(radix))
                .wrapping_add(u64::from(digit)),
        )
    })
}

const UNARY_BINDING_POWER: u8 = 22;

fn unary_operator(t: Type) -> Option<UnaryOperator> {
    Some(match t {
        Type::Minus => UnaryOperator::Negate,
        Type::Not => UnaryOperator::Not,
        Type::Hash => UnaryOperator::Length,
        Type::Tilde => UnaryOperator::BitwiseNot,
        _ => return None,
    })
}

/// The operator a token denotes in infix position, with its left and right
/// binding powers.
fn binary_operator(t: Type) -> Option<(BinaryOperator, u8, u8)> {
    use BinaryOperator::*;

    Some(match t {
        Type::Or => (Or, 1, 2),
        Type::And => (And, 3, 4),
        Type::LessThan => (LessThan, 5, 6),
        Type::GreaterThan => (GreaterThan, 5, 6),
        Type::LessThanOrEqual => (LessEqual, 5, 6),
        Type::GreaterThanOrEqual => (GreaterEqual, 5, 6),
        Type::Equal => (Equal, 5, 6),
        Type::NotEqual => (NotEqual, 5, 6),
        Type::Pipe => (BitwiseOr, 7, 8),
        Type::Tilde => (BitwiseXor, 9, 10),
        Type::Ampersand => (BitwiseAnd, 11, 12),
        Type::ShiftLeft => (ShiftLeft, 13, 14),
        Type::ShiftRight => (ShiftRight, 13, 14),
        Type::DoubleDot => (Concatenate, 16, 15),
        Type::Plus => (Add, 17, 18),
        Type::Minus => (Subtract, 17, 18),
        Type::Star => (Multiply, 19, 20),
        Type::Slash => (Divide, 19, 20),
        Type::DoubleSlash => (FloorDivide, 19, 20),
        Type::Percent => (Modulo, 19, 20),
        Type::Caret => (Power, 24, 23),
        _ => return None,
    })
}
