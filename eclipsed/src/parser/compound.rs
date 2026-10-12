//! Compound assignment (`a += 1`), shared by Luau and CfxLua.

use crate::ast::*;
use crate::lexer::{Span, Type};
use crate::parser::{ParseError, Parser};

pub(crate) fn compound_operator(token_type: Type) -> Option<CompoundOperator> {
    Some(match token_type {
        Type::PlusAssign => CompoundOperator::Add,
        Type::MinusAssign => CompoundOperator::Subtract,
        Type::StarAssign => CompoundOperator::Multiply,
        Type::SlashAssign => CompoundOperator::Divide,
        Type::DoubleSlashAssign => CompoundOperator::FloorDivide,
        Type::PercentAssign => CompoundOperator::Modulo,
        Type::CaretAssign => CompoundOperator::Power,
        Type::DoubleDotAssign => CompoundOperator::Concatenate,
        #[cfg(feature = "cfxlua")]
        Type::AmpersandAssign => CompoundOperator::BitwiseAnd,
        #[cfg(feature = "cfxlua")]
        Type::PipeAssign => CompoundOperator::BitwiseOr,
        #[cfg(feature = "cfxlua")]
        Type::ShiftLeftAssign => CompoundOperator::ShiftLeft,
        #[cfg(feature = "cfxlua")]
        Type::ShiftRightAssign => CompoundOperator::ShiftRight,
        _ => return None,
    })
}

impl<'source, 'arena, 'interner> Parser<'source, 'arena, 'interner> {
    /// Finishes `target op= value` with the cursor on the operator.
    pub(crate) fn parse_compound_assign(
        &mut self,
        start: u32,
        target: Expression<'arena>,
        operator: CompoundOperator,
    ) -> Result<Statement<'arena>, ParseError> {
        let target = Self::assignable(target)?;
        self.position += 1;
        let value = self.parse_expression()?;
        let span = Span::new(start, value.span().end);
        Ok(Statement::CompoundAssign {
            target,
            operator,
            value,
            span,
        })
    }
}
