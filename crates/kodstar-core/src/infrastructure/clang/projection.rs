// Purpose: Clang USR·호출 근거를 기존 구문 객체와 원본 호출 위치에 대응.
// Pattern: Streaming Projection.
// Usage: parser가 구문 사실을 통과시키고 완료 시 생성 정의를 추가.
// Related: crates/kodstar-core/src/infrastructure/clang/mod.rs, crates/kodstar-core/src/infrastructure/clang/generated.rs.

use super::Output;
use crate::domain::{
    diagnostics::Result,
    facts::{Binding, Fact},
    source::SourceSpan,
    symbols::SymbolKind,
};
use smallvec::SmallVec;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub(crate) struct Projection<'a> {
    pub(super) output: Arc<Output>,
    pub(super) source: &'a [u8],
    pub(super) emit: &'a mut dyn FnMut(Fact) -> Result<()>,
    pub(super) mapped: Vec<Option<u32>>,
    pub(super) calls: Vec<bool>,
    pub(super) owners: BTreeMap<String, u32>,
    pub(super) next: u32,
    definitions: BTreeMap<String, SmallVec<[usize; 2]>>,
    generated_spans: BTreeMap<(usize, usize), SmallVec<[usize; 2]>>,
    pub(super) generated_owners: BTreeSet<String>,
    call_sites: BTreeMap<(usize, usize), SmallVec<[usize; 2]>>,
}
impl<'a> Projection<'a> {
    pub fn new(
        output: Arc<Output>,
        source: &'a [u8],
        emit: &'a mut dyn FnMut(Fact) -> Result<()>,
    ) -> Self {
        let count = output.analysis.facts.len();
        let mut definitions: BTreeMap<String, SmallVec<[usize; 2]>> = BTreeMap::new();
        let mut generated_spans: BTreeMap<(usize, usize), SmallVec<[usize; 2]>> = BTreeMap::new();
        let mut generated_owners = BTreeSet::new();
        let mut call_sites: BTreeMap<(usize, usize), SmallVec<[usize; 2]>> = BTreeMap::new();
        for (index, fact) in output.analysis.facts.iter().enumerate() {
            if matches!(fact.kind, 1 | 3 | 4 | 5) {
                definitions
                    .entry(fact.name.to_owned())
                    .or_default()
                    .push(index);
            }
            if matches!(fact.kind, 1 | 3 | 4 | 5) && fact.generated {
                generated_spans
                    .entry((fact.begin, fact.end))
                    .or_default()
                    .push(index);
                generated_owners.insert(fact.usr.to_owned());
            }
            if fact.kind == 2 {
                call_sites
                    .entry((fact.begin, fact.end))
                    .or_default()
                    .push(index);
            }
        }
        Self {
            output,
            source,
            emit,
            mapped: vec![None; count],
            calls: vec![false; count],
            owners: BTreeMap::new(),
            next: 0,
            definitions,
            generated_spans,
            generated_owners,
            call_sites,
        }
    }
    pub fn push(&mut self, mut fact: Fact) -> Result<()> {
        let mut symbol = None;
        let mut call = None;
        match &mut fact {
            Fact::Scope(scope) => self.next = self.next.max(scope.local_id),
            Fact::Definition(definition) => {
                self.next = self.next.max(definition.local_id);
                if matches!(
                    definition.kind,
                    SymbolKind::Function
                        | SymbolKind::Method
                        | SymbolKind::Constructor
                        | SymbolKind::Enum
                        | SymbolKind::Field
                ) {
                    if let Some(indices) = self
                        .generated_spans
                        .get(&(definition.span.start_byte, definition.span.end_byte))
                        && indices.len() == 1
                    {
                        let native = self.output.analysis.facts.at(indices[0]);
                        if definition.name != native.name {
                            definition.name = native.name.to_owned();
                            definition.qualified_name = native.qualified.to_owned();
                        }
                    }
                    for &index in self.definitions.get(&definition.name).into_iter().flatten() {
                        let native = self.output.analysis.facts.at(index);
                        if definition.span.start_byte <= native.name_begin
                            && native.name_begin < definition.span.end_byte
                            && match native.kind {
                                1 => matches!(
                                    definition.kind,
                                    SymbolKind::Function
                                        | SymbolKind::Method
                                        | SymbolKind::Constructor
                                ),
                                3 => definition.kind == SymbolKind::Enum,
                                4 | 5 => definition.kind == SymbolKind::Field,
                                _ => false,
                            }
                        {
                            self.mapped[index] = Some(definition.local_id);
                            self.owners
                                .insert(native.usr.to_owned(), definition.local_id);
                            symbol = Some(index);
                            break;
                        }
                    }
                }
            }
            Fact::Use(usage) if usage.relation == crate::domain::relations::RelationKind::Calls => {
                for &index in self
                    .call_sites
                    .get(&(usage.span.start_byte, usage.span.end_byte))
                    .into_iter()
                    .flatten()
                {
                    let native = self.output.analysis.facts.at(index);
                    let owner_matches = self
                        .owners
                        .get(native.caller)
                        .is_none_or(|id| *id == usage.source);
                    let missing_generated = self.generated_owners.contains(native.caller)
                        && !self.owners.contains_key(native.caller);
                    if !self.calls[index] && owner_matches && !missing_generated {
                        self.calls[index] = true;
                        call = Some((index, usage.scope, usage.span));
                        break;
                    }
                }
            }
            _ => {}
        }
        if let Some((index, scope, span)) = call {
            self.metadata(
                "clang_call",
                Some(self.call_target(index)?),
                None,
                None,
                scope,
                span,
            )?;
        }
        (self.emit)(fact)?;
        if let Some(index) = symbol {
            let native = self.output.analysis.facts.at(index);
            let target = self.mapped[index];
            let location = SourceSpan::at(self.source, native.name_begin, native.name_end);
            self.metadata(
                "clang_symbol",
                Some(native.usr.to_owned()),
                native.defined.then(|| "definition".into()),
                target,
                0,
                location,
            )?;
        }
        Ok(())
    }
    pub(super) fn call_target(&self, index: usize) -> Result<String> {
        let native = self.output.analysis.facts.at(index);
        serde_json::to_string(&crate::domain::resolve::native::Target {
            usr: native.usr.into(),
            file: native.target_file.into(),
            position: native.target_position,
            name: native.name.into(),
        })
        .map_err(|e| {
            crate::domain::diagnostics::Error::caused_by("encode native callee location", e)
        })
    }
    pub(super) fn metadata(
        &mut self,
        kind: &'static str,
        module: Option<String>,
        imported_name: Option<String>,
        target: Option<u32>,
        scope: u32,
        span: SourceSpan,
    ) -> Result<()> {
        (self.emit)(Fact::Binding(Binding {
            scope,
            name: format!("@{kind}"),
            target,
            module,
            imported_name,
            type_name: None,
            kind,
            span,
        }))
    }
}
