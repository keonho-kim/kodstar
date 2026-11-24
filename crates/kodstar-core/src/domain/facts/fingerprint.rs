// Purpose: 원본 사실과 lookup 환경을 할당 없이 fingerprint에 누적.
// Pattern: Streaming Digest.
// Usage: worker가 추출 순서대로 누적하고 index는 완성된 두 digest를 저장.
// Related: crates/kodstar-core/src/domain/facts.rs, crates/kodstar-core/src/application/analysis/workers.rs.

use super::Fact;
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::domain::source::SourceSpan;
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug)]
pub(crate) struct Fingerprints {
    pub all: [u8; 32],
    pub environment: [u8; 32],
}

struct Digest(blake3::Hasher);
impl Default for Digest {
    fn default() -> Self {
        let mut hash = blake3::Hasher::new();
        hash.update(b"kodstar-syntax-facts-v2");
        Self(hash)
    }
}
impl Hasher for Digest {
    fn write(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
    fn finish(&self) -> u64 {
        let mut bytes = [0; 8];
        bytes.copy_from_slice(&self.0.finalize().as_bytes()[..8]);
        u64::from_le_bytes(bytes)
    }
}

#[derive(Default)]
pub(crate) struct Fingerprinter {
    all: Digest,
    environment: Digest,
}

impl Fingerprinter {
    pub(crate) fn push(&mut self, language: Language, fact: &mut Fact) {
        let original = match fact {
            Fact::Scope(s) if s.local_id != 0 => {
                let original = s.span;
                s.span.end_byte = s.span.start_byte;
                s.span.end_line = s.span.line;
                s.span.end_column = s.span.column;
                Some(original)
            }
            Fact::Scope(_) | Fact::Definition(super::Definition { local_id: 0, .. }) => {
                replace_span(fact, language)
            }
            _ => None,
        };
        // Scope ends are refreshed separately. Binding/use positions and definition
        // extents remain significant, including any visibility or derived fact changes.
        language.as_str().hash(&mut self.all);
        fact.hash(&mut self.all);
        restore_span(fact, original);
        if !matches!(fact, Fact::Use(u) if matches!(u.relation, RelationKind::Calls | RelationKind::References))
            && !matches!(fact, Fact::Binding(b) if b.kind == "clang_call")
        {
            let position = replace_span(fact, language);
            language.as_str().hash(&mut self.environment);
            fact.hash(&mut self.environment);
            restore_span(fact, position);
        }
    }

    pub(crate) fn finish(self) -> Fingerprints {
        Fingerprints {
            all: *self.all.0.finalize().as_bytes(),
            environment: *self.environment.0.finalize().as_bytes(),
        }
    }
}

fn replace_span(fact: &mut Fact, language: Language) -> Option<SourceSpan> {
    let span = match fact {
        Fact::Scope(s) => &mut s.span,
        Fact::Definition(d) => &mut d.span,
        Fact::Binding(b) if matches!(b.kind, "function" | "go_signature") => {
            if language == Language::Go {
                // Package functions are visible independently of their source order.
                return Some(std::mem::replace(&mut b.span, ZERO_SPAN));
            }
            let original = b.span;
            b.span.end_byte = b.span.start_byte;
            b.span.end_line = b.span.line;
            b.span.end_column = b.span.column;
            return Some(original);
        }
        _ => return None,
    };
    Some(std::mem::replace(span, ZERO_SPAN))
}

const ZERO_SPAN: SourceSpan = SourceSpan {
    start_byte: 0,
    end_byte: 0,
    line: 1,
    column: 1,
    end_line: 1,
    end_column: 1,
};

fn restore_span(fact: &mut Fact, original: Option<SourceSpan>) {
    if let Some(original) = original {
        match fact {
            Fact::Scope(s) => s.span = original,
            Fact::Definition(d) => d.span = original,
            Fact::Binding(b) => b.span = original,
            _ => {}
        }
    }
}
