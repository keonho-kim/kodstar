// Purpose: 분기별 지역 ID를 정규화하고 동일한 구문 사실을 제한된 메모리에서 병합.
// Pattern: Fact Reducer.
// Usage: conditional 추출이 각 투영의 사실을 전달하고 파일 종료 시 해제.
// Related: crates/kodstar-core/src/infrastructure/parse/conditional/mod.rs, crates/kodstar-core/src/domain/facts.rs.

use super::spill::Spill;
#[cfg(test)]
mod tests;
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, ErrorKind, Result};
use crate::domain::facts::{Definition, Fact, Scope};
use std::collections::{HashMap, HashSet};

pub(in crate::infrastructure::parse) struct Merger<'a> {
    definitions: HashMap<(Definition, Option<Scope>), u32>,
    scopes: HashMap<Scope, u32>,
    facts: HashSet<Fact>,
    mapping: HashMap<u32, u32>,
    pending: Option<Scope>,
    next: u32,
    bytes: usize,
    limit: usize,
    spill: Option<Spill>,
    cancel: &'a Cancellation,
    emit: &'a mut dyn FnMut(Fact) -> Result<()>,
}

impl<'a> Merger<'a> {
    pub(in crate::infrastructure::parse) fn new(
        limit: usize,
        cancel: &'a Cancellation,
        emit: &'a mut dyn FnMut(Fact) -> Result<()>,
    ) -> Self {
        Self {
            definitions: HashMap::new(),
            scopes: HashMap::new(),
            facts: HashSet::new(),
            mapping: HashMap::new(),
            pending: None,
            next: 0,
            bytes: 0,
            limit,
            spill: None,
            cancel,
            emit,
        }
    }

    pub(in crate::infrastructure::parse) fn begin(&mut self) -> Result<()> {
        self.flush_scope()?;
        self.mapping.clear();
        Ok(())
    }

    pub(in crate::infrastructure::parse) fn finish(&mut self) -> Result<()> {
        self.flush_scope()
    }

    fn id(&self, local: u32) -> Result<u32> {
        if local == 0 {
            return Ok(0);
        }
        self.mapping
            .get(&local)
            .copied()
            .ok_or_else(|| Error::operation("conditional fact references an unknown local object"))
    }

    fn allocate(&mut self) -> Result<u32> {
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| Error::new(ErrorKind::Limit, "too many conditional syntax objects"))?;
        Ok(self.next)
    }

    fn reserve(&mut self, bytes: usize) -> Result<()> {
        self.cancel.check()?;
        if self.spill.is_some() {
            return Ok(());
        }
        // Include hash-table spare capacity, ID maps and insertion reallocation overhead.
        self.bytes = self
            .bytes
            .saturating_add(bytes.saturating_add(256).saturating_mul(2));
        if self.bytes > self.limit {
            let spill = Spill::new(self.limit)?;
            for (key, value) in std::mem::take(&mut self.definitions) {
                spill.insert(0, &key, value, self.cancel)?;
            }
            for (key, value) in std::mem::take(&mut self.scopes) {
                spill.insert(1, &key, value, self.cancel)?;
            }
            for key in std::mem::take(&mut self.facts) {
                spill.insert(2, &key, 0, self.cancel)?;
            }
            self.spill = Some(spill);
        }
        Ok(())
    }

    fn flush_scope(&mut self) -> Result<()> {
        let Some(mut scope) = self.pending.take() else {
            return Ok(());
        };
        let local = scope.local_id;
        scope.local_id = 0;
        scope.parent = scope.parent.map(|id| self.id(id)).transpose()?;
        let existing = if let Some(spill) = &self.spill {
            spill.find(1, &scope, self.cancel)?
        } else {
            self.scopes.get(&scope).copied()
        };
        let canonical = if let Some(id) = existing {
            id
        } else {
            self.reserve(std::mem::size_of::<Scope>())?;
            let id = self.allocate()?;
            if let Some(spill) = &self.spill {
                spill.insert(1, &scope, id, self.cancel)?;
            } else {
                self.scopes.insert(scope.clone(), id);
            }
            scope.local_id = id;
            (self.emit)(Fact::Scope(scope))?;
            id
        };
        self.map(local, canonical)?;
        Ok(())
    }

    pub(in crate::infrastructure::parse) fn push(&mut self, mut fact: Fact) -> Result<()> {
        self.cancel.check()?;
        let paired = matches!(&fact, Fact::Definition(d) if self.pending.as_ref().is_some_and(|s| s.local_id == d.local_id));
        if !paired {
            self.flush_scope()?;
        }
        match fact {
            Fact::Scope(scope) => {
                self.pending = Some(scope);
                return Ok(());
            }
            Fact::Definition(mut definition) => {
                let local = definition.local_id;
                definition.local_id = 0;
                definition.owner = definition.owner.map(|id| self.id(id)).transpose()?;
                definition.scope = self.id(definition.scope)?;
                let mut scope = self.pending.take();
                if let Some(scope) = &mut scope {
                    scope.local_id = 0;
                    scope.parent = scope.parent.map(|id| self.id(id)).transpose()?;
                }
                let key = (definition, scope);
                let existing = if let Some(spill) = &self.spill {
                    spill.find(0, &key, self.cancel)?
                } else {
                    self.definitions.get(&key).copied()
                };
                let canonical = if let Some(id) = existing {
                    id
                } else {
                    self.reserve(
                        std::mem::size_of::<Fact>()
                            + key.0.payload_bytes()
                            + std::mem::size_of::<Scope>(),
                    )?;
                    let id = self.allocate()?;
                    if let Some(spill) = &self.spill {
                        spill.insert(0, &key, id, self.cancel)?;
                    } else {
                        self.definitions.insert(key.clone(), id);
                    }
                    let (mut definition, mut scope) = key;
                    if let Some(scope) = &mut scope {
                        scope.local_id = id;
                    }
                    if let Some(scope) = scope {
                        (self.emit)(Fact::Scope(scope))?;
                    }
                    definition.local_id = id;
                    (self.emit)(Fact::Definition(definition))?;
                    id
                };
                self.map(local, canonical)?;
                return Ok(());
            }
            Fact::Use(ref mut usage) => {
                usage.source = self.id(usage.source)?;
                usage.scope = self.id(usage.scope)?;
            }
            Fact::Binding(ref mut binding) => {
                binding.scope = self.id(binding.scope)?;
                binding.target = binding.target.map(|id| self.id(id)).transpose()?;
            }
            Fact::Module { .. } => {}
        }
        let existing = if let Some(spill) = &self.spill {
            spill.find(2, &fact, self.cancel)?.is_some()
        } else {
            self.facts.contains(&fact)
        };
        if !existing {
            self.reserve(fact.owned_bytes())?;
            if let Some(spill) = &self.spill {
                spill.insert(2, &fact, 0, self.cancel)?;
            } else {
                self.facts.insert(fact.clone());
            }
            (self.emit)(fact)?;
        }
        Ok(())
    }

    fn map(&mut self, local: u32, canonical: u32) -> Result<()> {
        if self.mapping.len().saturating_add(1).saturating_mul(64) > self.limit {
            return Err(Error::new(
                ErrorKind::Limit,
                "conditional local ID map exceeds its byte budget",
            ));
        }
        self.mapping.insert(local, canonical);
        Ok(())
    }
}
