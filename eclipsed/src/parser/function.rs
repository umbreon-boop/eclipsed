use crate::ast::*;
use crate::lexer::*;
use crate::parser::{ParseError, Parser};

impl<'source, 'arena, 'interner> Parser<'source, 'arena, 'interner> {
    pub(crate) fn parse_function_body(&mut self) -> Result<FunctionBody<'arena>, ParseError> {
        #[cfg(feature = "luau")]
        let attributes = self.pending_attributes.take().unwrap_or(&[]);
        #[cfg(not(feature = "luau"))]
        let attributes: &'arena [SpannedSymbol] = &[];

        // Starts at the `<` of the generic parameters or the `(`.
        let start = self.current_span().start;

        #[cfg(feature = "luau")]
        let type_parameters = if self.dialect.has_types() && self.consume(Type::LessThan) {
            self.parse_type_parameter_list()?
        } else {
            &[]
        };
        #[cfg(not(feature = "luau"))]
        let type_parameters: &'arena [TypeParameter<'arena>] = &[];

        self.expect(Type::LeftParenthesis, "'('")?;

        let mut parameters = Vec::new();
        let mut has_vararg = false;
        let mut vararg_type = None;

        if !self.check(Type::RightParenthesis) {
            loop {
                if self.consume(Type::TripleDot) {
                    has_vararg = true;
                    vararg_type = self.parse_optional_type_or_pack()?;
                    break;
                }
                parameters.push(self.parse_parameter("parameter name")?);
                if !self.consume(Type::Comma) {
                    break;
                }
            }
        }

        self.expect(Type::RightParenthesis, "')'")?;

        let returns = self.parse_optional_return_types()?;

        let body = self.parse_block()?;
        let end_token = self.expect(Type::End, "'end'")?;
        let span = Span::new(start, end_token.span.end);

        Ok(FunctionBody {
            attributes,
            type_parameters,
            parameters: self.alloc_list(parameters),
            has_vararg,
            vararg_type,
            returns,
            body,
            span,
        })
    }

    /// A name with an optional type annotation.
    pub(crate) fn parse_parameter(
        &mut self,
        expected: &'static str,
    ) -> Result<Parameter<'arena>, ParseError> {
        let name = self.expect(Type::Identifier, expected)?;
        Ok(Parameter {
            symbol: self.intern_token(name),
            name_span: name.span,
            type_annotation: self.parse_optional_type()?,
        })
    }
}
