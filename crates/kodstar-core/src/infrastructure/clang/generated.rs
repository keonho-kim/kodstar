// Purpose: grammar에 없는 매크로 생성 선언·호출을 의미 사실로 공개.
// Pattern: Projection Completion.
// Usage: 기존 AST 사실 전달을 마친 native projection이 호출.
// Related: crates/kodstar-core/src/infrastructure/clang/projection.rs, crates/kodstar-core/src/application/analysis/resolve/clang.rs.

use super::Projection;
use crate::domain::{
    diagnostics::{Error, ErrorKind, Result},
    facts::{Binding, Definition, Fact, Scope, SyntaxUse},
    relations::RelationKind,
    source::SourceSpan,
    symbols::SymbolKind,
};

impl Projection<'_> {
    pub fn finish(&mut self) -> Result<()> {
        let output = std::sync::Arc::clone(&self.output);
        for index in 0..output.analysis.facts.len() {
            let native = output.analysis.facts.at(index);
            if !matches!(native.kind, 1 | 3 | 4 | 5)
                || !native.generated
                || !native.defined
                || self.mapped[index].is_some()
            {
                continue;
            }
            self.next = self
                .next
                .checked_add(1)
                .ok_or_else(|| Error::new(ErrorKind::Limit, "native local ID overflow"))?;
            let id = self.next;
            let owner = self.owners.get(native.caller).copied().unwrap_or(0);
            let scope = if native.kind == 5 { owner } else { 0 };
            let kind = match native.kind {
                3 => SymbolKind::Enum,
                4 | 5 => SymbolKind::Field,
                _ => SymbolKind::Function,
            };
            let span = SourceSpan::at(self.source, native.begin, native.end.max(native.name_end));
            (self.emit)(Fact::Scope(Scope {
                local_id: id,
                parent: Some(scope),
                span,
            }))?;
            (self.emit)(Fact::Definition(Definition {
                local_id: id,
                owner: Some(owner),
                scope,
                name: native.name.to_owned(),
                qualified_name: native.qualified.to_owned(),
                kind,
                signature: String::new(),
                condition: None,
                span,
                exported: true,
            }))?;
            (self.emit)(Fact::Binding(Binding {
                scope,
                name: native.name.to_owned(),
                target: Some(id),
                module: None,
                imported_name: None,
                type_name: None,
                kind: match kind {
                    SymbolKind::Enum => "type",
                    SymbolKind::Field => "value",
                    _ => "function",
                },
                span,
            }))?;
            self.metadata(
                "clang_symbol",
                Some(native.usr.to_owned()),
                Some("definition".into()),
                Some(id),
                0,
                SourceSpan::at(self.source, native.name_begin, native.name_end),
            )?;
            self.mapped[index] = Some(id);
            self.owners.insert(native.usr.to_owned(), id);
        }
        for index in 0..self.output.analysis.facts.len() {
            let native = output.analysis.facts.at(index);
            if native.kind != 2 || self.calls[index] {
                continue;
            }
            if !self.generated_owners.contains(native.caller) {
                continue;
            }
            let Some(owner) = self.owners.get(native.caller).copied() else {
                continue;
            };
            let span = SourceSpan::at(self.source, native.begin, native.end);
            let expression = native.qualified.to_owned();
            let target = self.call_target(index)?;
            self.metadata("clang_call", Some(target), None, None, owner, span)?;
            (self.emit)(Fact::Use(SyntaxUse {
                source: owner,
                scope: owner,
                expression,
                relation: RelationKind::Calls,
                span,
                condition: None,
                type_only: false,
            }))?;
        }
        Ok(())
    }
}
