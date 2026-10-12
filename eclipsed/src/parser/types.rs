use crate::ast::*;
use crate::lexer::*;
use crate::parser::{ParseError, Parser};

impl<'source, 'arena, 'interner> Parser<'source, 'arena, 'interner> {
    pub(crate) fn parse_type_annotation(&mut self) -> Result<TypeAnnotation<'arena>, ParseError> {
        self.parse_type_union()
    }

    /// Return types: the elements of a tuple pack, or the one type.
    pub(crate) fn parse_return_types(
        &mut self,
    ) -> Result<&'arena [TypeAnnotation<'arena>], ParseError> {
        Ok(match self.parse_type_or_pack()? {
            TypeAnnotation::Tuple { elements, .. } => elements,
            other => std::slice::from_ref(self.arena.alloc(other)),
        })
    }

    /// A type or a type pack (`...T`, `T...`, `(T, U)`), as allowed after `->`,
    /// in generic arguments, defaults and return annotations.
    pub(crate) fn parse_type_or_pack(&mut self) -> Result<TypeAnnotation<'arena>, ParseError> {
        match self.peek_type() {
            Some(Type::TripleDot) => {
                let start = self.advance()?.span.start;
                let inner = self.parse_type_annotation()?;
                let span = Span::new(start, self.type_end(&inner));
                Ok(TypeAnnotation::Variadic {
                    inner: self.arena.alloc(inner),
                    span,
                })
            }
            Some(Type::Identifier) if self.peek_type_at(1) == Some(Type::TripleDot) => {
                let name_tok = self.advance()?;
                let dots = self.advance()?;
                Ok(TypeAnnotation::Pack {
                    name: self.intern_token(name_tok),
                    span: Span::new(name_tok.span.start, dots.span.end),
                })
            }
            _ => {
                self.pack_tuple_ok = self.check(Type::LeftParenthesis);
                let ty = self.parse_type_annotation();
                self.pack_tuple_ok = false;
                ty
            }
        }
    }

    fn parse_type_union(&mut self) -> Result<TypeAnnotation<'arena>, ParseError> {
        self.nested(Self::parse_type_union_inner)
    }

    fn parse_type_union_inner(&mut self) -> Result<TypeAnnotation<'arena>, ParseError> {
        let start = self.current_span().start;
        // Luau allows a redundant leading separator: `| A | B`.
        if self.dialect.has_types() {
            self.consume(Type::Pipe);
        }

        let mut variants = Vec::new();
        let first = self.parse_type_intersection()?;
        let mut end = self.type_end(&first);
        variants.push(first);

        while self.consume(Type::Pipe) {
            let next = self.parse_type_intersection()?;
            end = self.type_end(&next);
            variants.push(next);
        }

        if variants.len() == 1 {
            Ok(variants.pop().unwrap())
        } else {
            let span = Span::new(start, end);
            Ok(TypeAnnotation::Union {
                variants: self.alloc_list(variants),
                span,
            })
        }
    }

    /// `A & B`, or a single type in dialects without intersections.
    fn parse_type_intersection(&mut self) -> Result<TypeAnnotation<'arena>, ParseError> {
        let start = self.current_span().start;
        // Luau allows a redundant leading separator: `& A & B`.
        if self.dialect.has_types() {
            self.consume(Type::Ampersand);
        }

        let first = self.parse_type_optional()?;

        if self.dialect.has_intersection_types() && self.check(Type::Ampersand) {
            let mut parts = Vec::new();
            let mut end = self.type_end(&first);
            parts.push(first);
            while self.consume(Type::Ampersand) {
                let next = self.parse_type_optional()?;
                end = self.type_end(&next);
                parts.push(next);
            }
            let span = Span::new(start, end);
            return Ok(TypeAnnotation::Intersection {
                parts: self.alloc_list(parts),
                span,
            });
        }

        Ok(first)
    }

    fn parse_type_optional(&mut self) -> Result<TypeAnnotation<'arena>, ParseError> {
        // Taken before parsing: a grouped `(T)` reports only `T`'s span.
        let start = self.peek().map(|token| token.span.start);
        let mut ty = self.parse_type_primary()?;
        // Luau permits stacking: `string??`. Each `?` is its own node.
        while self.check(Type::QuestionMark) {
            let end = self.advance()?.span.end;
            let span = Span::new(start.unwrap_or_else(|| ty.span().start), end);
            ty = TypeAnnotation::Optional {
                inner: self.arena.alloc(ty),
                span,
            };
        }
        Ok(ty)
    }

    fn parse_type_primary(&mut self) -> Result<TypeAnnotation<'arena>, ParseError> {
        if self.dialect.has_types() {
            match self.peek_type() {
                Some(Type::LeftParenthesis) => return self.parse_luau_paren_type(),
                Some(Type::LeftCurlyBrace) => return self.parse_luau_table_type(),
                // Generic function type: `<T>(a: T) -> T`.
                Some(Type::LessThan) => {
                    let start = self.advance()?.span.start;
                    let type_parameters = self.parse_type_parameter_list()?;
                    if !self.check(Type::LeftParenthesis) {
                        return Err(self.expected("'(' after generic type parameters"));
                    }
                    let func = self.parse_luau_paren_type()?;
                    let TypeAnnotation::Function {
                        parameters,
                        returns,
                        span,
                        ..
                    } = func
                    else {
                        return Err(self.expected("'->' after generic function type parameters"));
                    };
                    return Ok(TypeAnnotation::Function {
                        type_parameters,
                        parameters,
                        returns,
                        span: Span::new(start, span.end),
                    });
                }
                _ => {}
            }
        }

        match self.peek_type() {
            Some(Type::String | Type::LongString) if self.dialect.has_singleton_types() => {
                let token = self.advance()?;
                let value = self.intern_token(token);
                Ok(TypeAnnotation::Singleton {
                    value: SingletonValue::String(value),
                    span: token.span,
                })
            }
            Some(Type::True | Type::False) if self.dialect.has_singleton_types() => {
                let token = self.advance()?;
                Ok(TypeAnnotation::Singleton {
                    value: SingletonValue::Boolean(token.token_type == Type::True),
                    span: token.span,
                })
            }
            Some(Type::Nil) => {
                let token = self.advance()?;
                Ok(TypeAnnotation::Named {
                    name: self.intern_token(token),
                    name_span: token.span,
                })
            }
            Some(Type::Identifier) => {
                let token = self.advance()?;

                if self.dialect.has_typeof_type()
                    && self.token_str(token) == "typeof"
                    && self.check(Type::LeftParenthesis)
                {
                    self.advance()?; // '('
                    let inner = self.parse_expression()?;
                    let close = self.expect(Type::RightParenthesis, "')'")?;
                    let span = Span::new(token.span.start, close.span.end);
                    let expression = self.arena.alloc(inner);
                    return Ok(TypeAnnotation::Typeof { expression, span });
                }

                let name = self.intern_token(token);
                let name_span = token.span;

                // Luau qualified type: `module.Foo`, `module.Foo<args>`.
                if self.dialect.has_types() && self.consume(Type::Dot) {
                    let inner_tok = self.expect(Type::Identifier, "type name")?;
                    let inner = self.spanned_symbol(inner_tok);
                    let (arguments, end) = if self.consume(Type::LessThan) {
                        let (args, end) = self.parse_generic_type_arguments()?;
                        (Some(args), end)
                    } else {
                        (None, inner_tok.span.end)
                    };
                    return Ok(TypeAnnotation::Qualified {
                        module: SpannedSymbol {
                            symbol: name,
                            span: name_span,
                        },
                        name: inner,
                        arguments,
                        span: Span::new(name_span.start, end),
                    });
                }

                if self.consume(Type::LessThan) {
                    let (arguments, end) = self.parse_generic_type_arguments()?;
                    Ok(TypeAnnotation::Generic {
                        name,
                        name_span,
                        arguments,
                        span: Span::new(name_span.start, end),
                    })
                } else {
                    Ok(TypeAnnotation::Named { name, name_span })
                }
            }
            _ => Err(self.expected("type annotation")),
        }
    }

    /// A declared generic parameter list after its `<`, through the `>`.
    pub(crate) fn parse_type_parameter_list(
        &mut self,
    ) -> Result<&'arena [TypeParameter<'arena>], ParseError> {
        let mut params = Vec::new();
        let (mut seen_pack, mut seen_default) = (false, false);
        loop {
            let name_tok = self.expect(Type::Identifier, "type parameter name")?;
            let symbol = self.intern_token(name_tok);

            let is_pack = self.dialect.has_types() && self.consume(Type::TripleDot);

            let default = if self.dialect.has_types() && self.consume(Type::Assign) {
                Some(self.parse_type_or_pack()?)
            } else {
                None
            };

            // Packs follow single types, a pack defaults to a pack, and
            // defaults, once started, continue.
            if seen_pack && !is_pack {
                return Err(ParseError::Expected {
                    expected: "type pack (packs must come after single type parameters)",
                    got: self.token_str(name_tok).to_string(),
                    span: name_tok.span,
                });
            }
            seen_pack |= is_pack;

            match &default {
                Some(value) => {
                    if is_pack
                        && !matches!(
                            value,
                            TypeAnnotation::Tuple { .. }
                                | TypeAnnotation::Variadic { .. }
                                | TypeAnnotation::Pack { .. }
                        )
                    {
                        return Err(ParseError::Expected {
                            expected: "type pack as the default of a type pack parameter",
                            got: "single type".to_string(),
                            span: value.span(),
                        });
                    }
                    seen_default = true;
                }
                None if seen_default => {
                    return Err(ParseError::Expected {
                        expected: "default value (parameters after a defaulted one \
                                       must also have defaults)",
                        got: self.token_str(name_tok).to_string(),
                        span: name_tok.span,
                    });
                }
                None => {}
            }

            params.push(TypeParameter {
                symbol,
                name_span: name_tok.span,
                is_pack,
                default,
            });
            if !self.consume(Type::Comma) {
                break;
            }
        }
        self.expect_close_angle()?;
        Ok(self.alloc_list(params))
    }

    /// Generic type arguments after the `<`, with the end of the closing `>`.
    fn parse_generic_type_arguments(
        &mut self,
    ) -> Result<(&'arena [TypeAnnotation<'arena>], u32), ParseError> {
        let args = if self.check_close_angle() {
            Vec::new()
        } else {
            self.comma_list(Self::parse_type_or_pack)?
        };
        let end = self.expect_close_angle()?;
        Ok((self.alloc_list(args), end))
    }

    /// A function type, a grouped type or, where packs are allowed, a tuple
    /// pack, starting at `(`.
    fn parse_luau_paren_type(&mut self) -> Result<TypeAnnotation<'arena>, ParseError> {
        let tuple_ok = std::mem::take(&mut self.pack_tuple_ok);
        let start = self.expect(Type::LeftParenthesis, "'('")?.span.start;

        let mut params = if self.check(Type::RightParenthesis) {
            Vec::new()
        } else {
            self.comma_list(|this| {
                Ok(match (this.peek_type(), this.peek_type_at(1)) {
                    // Named parameter: `cost: number`.
                    (Some(Type::Identifier), Some(Type::Colon)) => {
                        let name = this.advance()?;
                        this.advance()?;
                        FunctionTypeParameter {
                            name: Some(this.spanned_symbol(name)),
                            type_annotation: this.parse_type_annotation()?,
                        }
                    }
                    (Some(Type::TripleDot), _)
                    | (Some(Type::Identifier), Some(Type::TripleDot)) => FunctionTypeParameter {
                        name: None,
                        type_annotation: this.parse_type_or_pack()?,
                    },
                    _ => FunctionTypeParameter {
                        name: None,
                        type_annotation: this.parse_type_annotation()?,
                    },
                })
            })?
        };
        let close = self.expect(Type::RightParenthesis, "')'")?;

        if self.consume(Type::Arrow) {
            let returns = self.parse_return_types()?;
            // `-> ()` has no last element to take an end from.
            let end = self.last_span_end(start);
            return Ok(TypeAnnotation::Function {
                type_parameters: &[],
                parameters: self.alloc_list(params),
                returns,
                span: Span::new(start, end),
            });
        }

        if params.len() == 1
            && params[0].name.is_none()
            && !matches!(
                params[0].type_annotation,
                TypeAnnotation::Variadic { .. } | TypeAnnotation::Pack { .. }
            )
        {
            let param = params.pop().unwrap();
            return Ok(param.type_annotation);
        }

        if tuple_ok {
            let elements = self
                .arena
                .alloc_slice_fill_iter(params.into_iter().map(|p| p.type_annotation));
            return Ok(TypeAnnotation::Tuple {
                elements,
                span: Span::new(start, close.span.end),
            });
        }

        Err(self.expected("'->' after parenthesized parameter types"))
    }

    /// `{}`, the array shorthand `{ T }`, or a field list.
    fn parse_luau_table_type(&mut self) -> Result<TypeAnnotation<'arena>, ParseError> {
        let start = self.expect(Type::LeftCurlyBrace, "'{'")?.span.start;

        if self.check(Type::RightCurlyBrace) {
            let end = self.advance()?.span.end;
            return Ok(TypeAnnotation::Struct {
                fields: &[],
                span: Span::new(start, end),
            });
        }

        // Array shorthand: the body is a single type with no key syntax.
        if !self.peek_is_table_type_field() {
            let element = self.parse_type_annotation()?;
            let end_tok = self.expect(Type::RightCurlyBrace, "'}'")?;
            return Ok(TypeAnnotation::Array {
                element: self.arena.alloc(element),
                span: Span::new(start, end_tok.span.end),
            });
        }

        let mut fields = Vec::new();
        while !self.check(Type::RightCurlyBrace) {
            let access = self.parse_table_type_access();

            let key = if self.consume(Type::LeftSquareBracket) {
                let key_ty = self.parse_type_annotation()?;
                self.expect(Type::RightSquareBracket, "']'")?;
                TableTypeKey::Indexer(self.arena.alloc(key_ty))
            } else {
                let name_tok = self.expect(Type::Identifier, "field name")?;
                TableTypeKey::Name(self.spanned_symbol(name_tok))
            };
            self.expect(Type::Colon, "':'")?;
            let value = self.parse_type_annotation()?;
            fields.push(TableTypeField { access, key, value });

            if !self.consume(Type::Comma) && !self.consume(Type::Semicolon) {
                break;
            }
        }
        let end_tok = self.expect(Type::RightCurlyBrace, "'}'")?;
        let span = Span::new(start, end_tok.span.end);

        // Keep the dedicated Map shape for a lone unqualified indexer.
        if fields.len() == 1
            && fields[0].access.is_none()
            && matches!(fields[0].key, TableTypeKey::Indexer(_))
        {
            let field = fields.pop().unwrap();
            let TableTypeKey::Indexer(key) = field.key else {
                unreachable!()
            };
            return Ok(TypeAnnotation::Map {
                key,
                value: self.arena.alloc(field.value),
                span,
            });
        }

        Ok(TypeAnnotation::Struct {
            fields: self.alloc_list(fields),
            span,
        })
    }

    /// Whether a keyed field, rather than an array element type, is ahead.
    fn peek_is_table_type_field(&self) -> bool {
        match self.peek_type() {
            Some(Type::LeftSquareBracket) => true,
            Some(Type::Identifier) => {
                let next = self.peek_type_at(1);
                if next == Some(Type::Colon) {
                    return true;
                }
                // `read x: T` / `write [K]: V`
                matches!(next, Some(Type::Identifier | Type::LeftSquareBracket))
                    && matches!(self.token_str(self.peek().unwrap()), "read" | "write")
            }
            _ => false,
        }
    }

    /// Consumes a `read`/`write` modifier.
    fn parse_table_type_access(&mut self) -> Option<TableAccess> {
        let token = self.peek()?;
        if token.token_type != Type::Identifier {
            return None;
        }
        let next = self.peek_type_at(1);
        if !matches!(next, Some(Type::Identifier | Type::LeftSquareBracket)) {
            return None;
        }
        let access = match self.token_str(token) {
            "read" => TableAccess::Read,
            "write" => TableAccess::Write,
            _ => return None,
        };
        self.position += 1;
        Some(access)
    }
}
