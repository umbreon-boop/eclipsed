//! Module-level declarations, flattened into [`EclDecl`] rows.

use eclipsed::api;
use eclipsed::declarations::{DeclarationKind, NameJoin};

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EclRecord {
    /// Starts a declaration; `kind` is an [`EclDeclKind`], the range its span.
    Decl = 0,
    /// A name segment; `kind` is an [`EclNameJoin`].
    Name = 1,
    Param = 2,
    /// A `...` parameter, with an empty range.
    Vararg = 3,
    /// A doc comment, verbatim.
    Doc = 4,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EclDeclKind {
    Function = 0,
    Field = 1,
    Type = 2,
    TypeFunction = 3,
}

/// How a name segment joins the one before it.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EclNameJoin {
    /// `.`, or nothing for the first segment.
    Dot = 0,
    /// `:`
    Method = 1,
}

/// One row of a declaration listing.
///
/// Each declaration is a `DECL` row followed by its `NAME`, `PARAM`,
/// `VARARG` and `DOC` rows, in that order. `kind` is zero on rows that
/// document no enum for it.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EclDecl {
    pub record: EclRecord,
    pub kind: u32,
    pub start: u32,
    pub end: u32,
}

pub(crate) fn collect(chunk: &api::Chunk<'_>) -> Vec<EclDecl> {
    let mut rows = Vec::new();
    let mut row = |record, kind: u32, start, end| {
        rows.push(EclDecl {
            record,
            kind,
            start,
            end,
        })
    };
    for decl in chunk.declarations() {
        let kind = match decl.kind {
            DeclarationKind::Function => EclDeclKind::Function,
            DeclarationKind::Field => EclDeclKind::Field,
            DeclarationKind::Type => EclDeclKind::Type,
            DeclarationKind::TypeFunction => EclDeclKind::TypeFunction,
        };
        row(EclRecord::Decl, kind as u32, decl.span.start, decl.span.end);
        for (join, span) in decl.name {
            let join = match join {
                NameJoin::Dot => EclNameJoin::Dot,
                NameJoin::Method => EclNameJoin::Method,
            };
            row(EclRecord::Name, join as u32, span.start, span.end);
        }
        for span in decl.parameters {
            row(EclRecord::Param, 0, span.start, span.end);
        }
        if decl.vararg {
            row(EclRecord::Vararg, 0, 0, 0);
        }
        for span in decl.doc {
            row(EclRecord::Doc, 0, span.start, span.end);
        }
    }
    rows
}
