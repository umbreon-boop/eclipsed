use crate::ast::*;
use crate::lexer::*;
use crate::parser::{ParseError, Parser};

impl<'source, 'arena, 'interner> Parser<'source, 'arena, 'interner> {
    pub fn parse_statement(&mut self) -> Result<Statement<'arena>, ParseError> {
        let Some(&token) = self.peek() else {
            return Err(self.unexpected());
        };
        let start = token.span.start;

        #[cfg(feature = "luau")]
        if let Some(statement) = self.parse_luau_statement(&token)? {
            return Ok(statement);
        }

        match token.token_type {
            Type::Local => self.parse_local(start),
            Type::Function => self.parse_function(start),
            Type::Do => self.parse_do(start),
            Type::If => self.parse_if(start),
            Type::While => self.parse_while(start),
            Type::Repeat => self.parse_repeat(start),
            Type::For => self.parse_for(start),
            Type::Return => self.parse_return(start),
            Type::Break => {
                self.position += 1;
                Ok(Statement::Break { span: token.span })
            }
            Type::Goto => self.parse_goto(start),
            Type::DoubleColon => self.parse_label(start),
            _ => self.parse_expr_or_assign(start),
        }
    }

    pub(crate) fn parse_local(&mut self, start: u32) -> Result<Statement<'arena>, ParseError> {
        self.position += 1;

        if self.consume(Type::Function) {
            let name = self.expect(Type::Identifier, "function name")?;
            let symbol = self.intern_token(name);
            let body = self.parse_function_body()?;
            return Ok(Statement::LocalFunction {
                symbol,
                name_span: name.span,
                span: Span::new(start, body.span.end),
                body: self.arena.alloc(body),
            });
        }

        let variables = self.parse_local_varlist()?;

        #[cfg(feature = "cfxlua")]
        if self.dialect.has_in_unpacking() && self.consume(Type::In) {
            let source = self.parse_expression()?;
            return Ok(Statement::InUnpack {
                variables,
                span: Span::new(start, source.span().end),
                source,
            });
        }

        let values = if self.consume(Type::Assign) {
            self.parse_expression_list()?
        } else {
            &[]
        };
        Ok(Statement::Local {
            variables,
            values,
            span: Span::new(start, self.last_span_end(start)),
        })
    }

    /// A comma-separated list of `local`-style names with their attributes
    /// and annotations.
    pub(crate) fn parse_local_varlist(
        &mut self,
    ) -> Result<&'arena [LocalVariable<'arena>], ParseError> {
        let variables = self.comma_list(Self::parse_local_variable)?;
        Ok(self.alloc_list(variables))
    }

    fn parse_local_variable(&mut self) -> Result<LocalVariable<'arena>, ParseError> {
        let name = self.expect(Type::Identifier, "variable name")?;
        let attribute = if self.consume(Type::LessThan) {
            let token = self.expect(Type::Identifier, "variable attribute")?;
            let attribute = match self.token_str(token) {
                "close" => VariableAttribute::Close,
                "const" => VariableAttribute::Const,
                other => {
                    return Err(ParseError::InvalidAttribute {
                        name: other.to_owned(),
                        span: token.span,
                    });
                }
            };
            self.expect(Type::GreaterThan, "'>'")?;
            Some(attribute)
        } else {
            None
        };
        Ok(LocalVariable {
            symbol: self.intern_token(name),
            name_span: name.span,
            type_annotation: self.parse_optional_type()?,
            attribute,
        })
    }

    pub(crate) fn parse_function(&mut self, start: u32) -> Result<Statement<'arena>, ParseError> {
        self.position += 1;

        let mut path = Vec::new();
        let first = self.expect(Type::Identifier, "function name")?;
        path.push(self.spanned_symbol(first));
        while self.consume(Type::Dot) {
            let segment = self.expect_field_name("field name")?;
            path.push(self.spanned_symbol(segment));
        }
        let method = if self.consume(Type::Colon) {
            let method = self.expect_field_name("method name")?;
            Some(self.spanned_symbol(method))
        } else {
            None
        };

        let body = self.parse_function_body()?;
        Ok(Statement::Function {
            name: FunctionName {
                path: self.alloc_list(path),
                method,
            },
            span: Span::new(start, body.span.end),
            body: self.arena.alloc(body),
        })
    }

    pub(crate) fn spanned_symbol(&mut self, token: &Token) -> SpannedSymbol {
        SpannedSymbol {
            symbol: self.intern_token(token),
            span: token.span,
        }
    }

    /// Parses a block and the `end` closing it, returning the `end` offset.
    fn parse_block_end(&mut self) -> Result<(&'arena [Statement<'arena>], u32), ParseError> {
        let body = self.parse_block()?;
        let end = self.expect(Type::End, "'end'")?.span.end;
        Ok((body, end))
    }

    fn parse_do(&mut self, start: u32) -> Result<Statement<'arena>, ParseError> {
        self.position += 1;
        let (body, end) = self.parse_block_end()?;
        Ok(Statement::Do {
            body,
            span: Span::new(start, end),
        })
    }

    fn parse_if(&mut self, start: u32) -> Result<Statement<'arena>, ParseError> {
        self.position += 1;
        #[cfg(feature = "luau")]
        let (binding, condition) = self.parse_luau_if_condition()?;
        #[cfg(not(feature = "luau"))]
        let condition = self.parse_expression()?;
        self.expect(Type::Then, "'then'")?;
        let then_block = self.parse_block()?;

        let mut elseifs = Vec::new();
        while self.consume(Type::Elseif) {
            #[cfg(feature = "luau")]
            let (binding, condition) = self.parse_luau_if_condition()?;
            #[cfg(not(feature = "luau"))]
            let condition = self.parse_expression()?;
            self.expect(Type::Then, "'then'")?;
            let body = self.parse_block()?;
            elseifs.push(ElseIf {
                #[cfg(feature = "luau")]
                binding,
                condition,
                body,
            });
        }
        let else_block = if self.consume(Type::Else) {
            Some(self.parse_block()?)
        } else {
            None
        };

        let end = self.expect(Type::End, "'end'")?.span.end;
        Ok(Statement::If {
            #[cfg(feature = "luau")]
            binding,
            condition,
            then_block,
            elseifs: self.alloc_list(elseifs),
            else_block,
            span: Span::new(start, end),
        })
    }

    fn parse_while(&mut self, start: u32) -> Result<Statement<'arena>, ParseError> {
        self.position += 1;
        let condition = self.parse_expression()?;
        self.expect(Type::Do, "'do'")?;
        let (body, end) = self.parse_block_end()?;
        Ok(Statement::While {
            condition,
            body,
            span: Span::new(start, end),
        })
    }

    fn parse_repeat(&mut self, start: u32) -> Result<Statement<'arena>, ParseError> {
        self.position += 1;
        let body = self.parse_block()?;
        self.expect(Type::Until, "'until'")?;
        let until = self.parse_expression()?;
        Ok(Statement::Repeat {
            body,
            span: Span::new(start, until.span().end),
            until,
        })
    }

    fn parse_for(&mut self, start: u32) -> Result<Statement<'arena>, ParseError> {
        self.position += 1;
        let first = self.parse_parameter("variable name")?;

        if self.consume(Type::Assign) {
            let initial = self.parse_expression()?;
            self.expect(Type::Comma, "','")?;
            let limit = self.parse_expression()?;
            let step = if self.consume(Type::Comma) {
                Some(self.parse_expression()?)
            } else {
                None
            };
            self.expect(Type::Do, "'do'")?;
            let (body, end) = self.parse_block_end()?;
            return Ok(Statement::NumericFor {
                variable: first.symbol,
                variable_span: first.name_span,
                variable_type: first.type_annotation,
                start: initial,
                limit,
                step,
                body,
                span: Span::new(start, end),
            });
        }

        let mut variables = vec![first];
        while self.consume(Type::Comma) {
            variables.push(self.parse_parameter("variable name")?);
        }
        self.expect(Type::In, "'in'")?;
        let iterators = self.parse_expression_list()?;
        self.expect(Type::Do, "'do'")?;
        let (body, end) = self.parse_block_end()?;
        Ok(Statement::GenericFor {
            variables: self.alloc_list(variables),
            iterators,
            body,
            span: Span::new(start, end),
        })
    }

    fn parse_return(&mut self, start: u32) -> Result<Statement<'arena>, ParseError> {
        self.position += 1;
        let values = if self.is_block_end() || self.check(Type::Semicolon) {
            &[]
        } else {
            self.parse_expression_list()?
        };
        self.consume(Type::Semicolon);
        Ok(Statement::Return {
            values,
            span: Span::new(start, self.last_span_end(start)),
        })
    }

    fn parse_goto(&mut self, start: u32) -> Result<Statement<'arena>, ParseError> {
        self.position += 1;
        let label = self.expect(Type::Identifier, "label name")?;
        Ok(Statement::Goto {
            label: self.intern_token(label),
            label_span: label.span,
            span: Span::new(start, label.span.end),
        })
    }

    fn parse_label(&mut self, start: u32) -> Result<Statement<'arena>, ParseError> {
        self.position += 1;
        let name = self.expect(Type::Identifier, "label name")?;
        let end = self.expect(Type::DoubleColon, "'::'")?.span.end;
        Ok(Statement::Label {
            symbol: self.intern_token(name),
            name_span: name.span,
            span: Span::new(start, end),
        })
    }

    fn parse_expr_or_assign(&mut self, start: u32) -> Result<Statement<'arena>, ParseError> {
        let first = self.parse_statement_expression()?;

        #[cfg(feature = "compound")]
        if self.dialect.has_compound_assignment()
            && let Some(operator) = self
                .peek_type()
                .and_then(crate::parser::compound::compound_operator)
        {
            return self.parse_compound_assign(start, first, operator);
        }

        if !self.check(Type::Assign) && !self.check(Type::Comma) {
            if !matches!(
                first,
                Expression::Call { .. } | Expression::MethodCall { .. }
            ) {
                return Err(ParseError::Expected {
                    expected: "statement",
                    got: "expression".to_owned(),
                    span: first.span(),
                });
            }
            return Ok(Statement::Expression {
                span: Span::new(start, first.span().end),
                expr: self.arena.alloc(first),
            });
        }

        let mut targets = Vec::new();
        targets.push(Self::assignable(first)?);
        while self.consume(Type::Comma) {
            let target = self.parse_statement_expression()?;
            targets.push(Self::assignable(target)?);
        }
        self.expect(Type::Assign, "'='")?;
        let values = self.parse_expression_list()?;
        Ok(Statement::Assign {
            targets: self.alloc_list(targets),
            span: Span::new(start, self.last_span_end(start)),
            values,
        })
    }

    /// Accepts only Lua `var`s: a name, a field or an index. `(a) = 1` is a
    /// `prefixexp` but not a `var`, so it is rejected too.
    pub(crate) fn assignable(target: Expression<'arena>) -> Result<Expression<'arena>, ParseError> {
        match target {
            Expression::Identifier { .. } | Expression::Field { .. } | Expression::Index { .. } => {
                Ok(target)
            }
            _ => Err(ParseError::Expected {
                expected: "a variable, field or index to assign to",
                got: "expression".to_owned(),
                span: target.span(),
            }),
        }
    }

    /// Parses statements up to a block terminator, discarding `;` separators.
    pub(crate) fn parse_block(&mut self) -> Result<&'arena [Statement<'arena>], ParseError> {
        self.nested(|this| {
            let mut statements = Vec::new();
            while !this.is_block_end() {
                if this.consume(Type::Semicolon) {
                    continue;
                }
                let statement = this.parse_statement()?;
                let terminates = statement.is_block_terminator();
                statements.push(statement);
                if terminates {
                    while this.consume(Type::Semicolon) {}
                    break;
                }
            }
            Ok(this.alloc_list(statements))
        })
    }

    #[inline]
    pub(crate) fn is_block_end(&self) -> bool {
        matches!(
            self.peek_type(),
            None | Some(Type::End | Type::Else | Type::Elseif | Type::Until)
        )
    }
}

impl Statement<'_> {
    /// `return`, and Luau `continue`, must end their block.
    fn is_block_terminator(&self) -> bool {
        match self {
            Statement::Return { .. } => true,
            #[cfg(feature = "luau")]
            Statement::Continue { .. } => true,
            _ => false,
        }
    }
}
