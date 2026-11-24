// Purpose: 기존 사실의 위치 보정 동등성과 새 선언의 독립 소유 범위 증명.
// Pattern: Specification.
// Usage: worker 사전 추출·재생이 관찰하고 index가 실제 lookup 소비자를 확인.
// Related: crates/kodstar-core/src/domain/facts/append/source.rs, crates/kodstar-core/src/domain/facts/fingerprint.rs.

use super::{Candidate, Insertion, Membership, membership, set_span, span};
use crate::domain::{
    facts::{Fact, Fingerprinter},
    languages::Language,
    relations::RelationKind,
    symbols::SymbolKind,
};

pub(crate) struct Tracker {
    source: Insertion,
    fingerprint: Fingerprinter,
    previous: [u8; 32],
    names: Vec<String>,
    declarations: Vec<u32>,
    scopes: Vec<u32>,
    containers: Vec<u32>,
    scala_classes: Vec<u32>,
    scala_objects: Vec<u32>,
    existing_modules: Vec<u32>,
    bindings: Vec<u32>,
    uses: Vec<u32>,
    budget: usize,
    valid: bool,
}
impl Tracker {
    pub fn new(source: Insertion, previous: [u8; 32], budget: usize) -> Self {
        Self {
            source,
            fingerprint: Fingerprinter::default(),
            previous,
            names: Vec::new(),
            declarations: Vec::new(),
            scopes: Vec::new(),
            containers: Vec::new(),
            scala_classes: Vec::new(),
            scala_objects: Vec::new(),
            existing_modules: Vec::new(),
            bindings: Vec::new(),
            uses: Vec::new(),
            budget,
            valid: true,
        }
    }
    pub fn push(&mut self, language: Language, fact: &mut Fact) {
        if !self.valid {
            return;
        }
        match membership(fact, &self.source.range) {
            Membership::Invalid => {
                self.valid = false;
                return;
            }
            Membership::Existing => {
                if let Fact::Binding(b) = fact
                    && b.kind == "scala_object"
                    && let Some(id) = b.target
                {
                    self.existing_modules.push(id);
                }
                if let Fact::Definition(d) = fact
                    && d.kind == SymbolKind::Module
                {
                    self.existing_modules.push(d.local_id);
                }
                let original = span(fact);
                if let Some(span) = original {
                    set_span(fact, self.source.original(span));
                }
                self.fingerprint.push(language, fact);
                if let Some(span) = original {
                    set_span(fact, span);
                }
                return;
            }
            Membership::Added => {}
        }
        self.valid &= match fact {
            Fact::Definition(d) => {
                let owner = d.owner.unwrap_or(0);
                let inherited = self.containers.contains(&owner);
                let root = owner == 0 || self.existing_modules.contains(&owner);
                let kind = matches!(
                    d.kind,
                    SymbolKind::Function | SymbolKind::Method | SymbolKind::Module
                ) || matches!(
                    language,
                    Language::Java | Language::CSharp | Language::Scala
                ) && d.kind == SymbolKind::Class;
                if !(kind && (root || inherited) && d.condition.is_none()) {
                    return self.valid = false;
                }
                if root {
                    let name = d
                        .name
                        .rsplit([':', '.'])
                        .next()
                        .unwrap_or("")
                        .split('/')
                        .next()
                        .unwrap_or("");
                    let valid_name = name
                        .bytes()
                        .next()
                        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
                        && name
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"_-!?".contains(&b));
                    if !valid_name || implicit(name) {
                        return self.valid = false;
                    }
                    if !self.names.iter().any(|n| n == name) {
                        self.names.push(name.to_owned());
                    }
                }
                self.declarations.push(d.local_id);
                if language == Language::Scala && d.kind == SymbolKind::Class {
                    self.scala_classes.push(d.local_id);
                }
                if matches!(d.kind, SymbolKind::Class | SymbolKind::Module) {
                    self.containers.push(d.local_id);
                }
                true
            }
            Fact::Scope(s) => {
                self.scopes.push(s.local_id);
                true
            }
            Fact::Binding(b) => {
                if b.kind == "scala_given" {
                    return self.valid = false;
                }
                if b.kind == "scala_object"
                    && let Some(id) = b.target
                {
                    self.scala_objects.push(id);
                }
                if b.kind.starts_with("hono_")
                    || b.kind.starts_with("http_")
                    || b.kind.starts_with("htmx_")
                {
                    return self.valid = false;
                }
                if !self.scopes.contains(&b.scope) {
                    if let Some(target) = b.target {
                        self.bindings.push(target);
                    } else {
                        return self.valid = false;
                    }
                }
                true
            }
            Fact::Use(u) => {
                self.uses.push(u.source);
                matches!(
                    u.relation,
                    RelationKind::Calls
                        | RelationKind::References
                        | RelationKind::UsesType
                        | RelationKind::Imports
                        | RelationKind::Reexports
                        | RelationKind::Includes
                )
            }
            Fact::Module { .. } => false,
        };
        let bytes = self.names.iter().map(String::capacity).sum::<usize>()
            + self.names.capacity() * std::mem::size_of::<String>()
            + [
                self.declarations.capacity(),
                self.scopes.capacity(),
                self.containers.capacity(),
                self.scala_classes.capacity(),
                self.scala_objects.capacity(),
                self.existing_modules.capacity(),
                self.bindings.capacity(),
                self.uses.capacity(),
            ]
            .iter()
            .sum::<usize>()
                * 4;
        self.valid &= bytes <= self.budget;
    }
    pub fn finish(self) -> Option<Candidate> {
        (self.valid
            && self
                .scala_classes
                .iter()
                .all(|id| self.scala_objects.contains(id))
            && !self.names.is_empty()
            && self.fingerprint.finish().all == self.previous
            && self
                .bindings
                .iter()
                .chain(&self.uses)
                .all(|id| self.declarations.contains(id)))
        .then_some(Candidate {
            range: self.source.range,
            names: self.names,
        })
    }
}

fn implicit(name: &str) -> bool {
    // Implicit dispatch need not spell its selected method at a use site.
    name.starts_with("__") && name.ends_with("__")
        || matches!(
            name,
            "constructor"
                | "initialize"
                | "method_missing"
                | "respond_to_missing?"
                | "const_missing"
                | "call"
                | "callAsFunction"
                | "apply"
                | "unapply"
                | "new"
                | "init"
                | "deinit"
        )
}
