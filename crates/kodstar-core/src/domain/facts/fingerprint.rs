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
    pub all: String,
    pub environment: String,
}

struct Digest(blake3::Hasher);
impl Default for Digest {
    fn default() -> Self {
        let mut hash = blake3::Hasher::new();
        hash.update(b"kodstar-syntax-facts-v1");
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
        let root = match fact {
            Fact::Scope(s) => s.local_id == 0,
            Fact::Definition(d) => d.local_id == 0,
            _ => false,
        };
        // File length is refreshed independently; every other fact field remains significant.
        let original = root.then(|| replace_span(fact)).flatten();
        language.as_str().hash(&mut self.all);
        fact.hash(&mut self.all);
        if !matches!(fact, Fact::Use(u) if matches!(u.relation, RelationKind::Calls | RelationKind::References))
        {
            let position = replace_span(fact);
            language.as_str().hash(&mut self.environment);
            fact.hash(&mut self.environment);
            restore_span(fact, position);
        }
        restore_span(fact, original);
    }

    pub(crate) fn finish(self) -> Fingerprints {
        Fingerprints {
            all: self.all.0.finalize().to_hex().to_string(),
            environment: self.environment.0.finalize().to_hex().to_string(),
        }
    }
}

fn replace_span(fact: &mut Fact) -> Option<SourceSpan> {
    let span = match fact {
        Fact::Scope(s) => &mut s.span,
        Fact::Definition(d) => &mut d.span,
        Fact::Binding(b) if matches!(b.kind, "function" | "go_signature") => {
            let original = b.span;
            b.span.end_byte = b.span.start_byte;
            b.span.end_line = b.span.line;
            b.span.end_column = b.span.column;
            return Some(original);
        }
        _ => return None,
    };
    Some(std::mem::replace(
        span,
        SourceSpan {
            start_byte: 0,
            end_byte: 0,
            line: 1,
            column: 1,
            end_line: 1,
            end_column: 1,
        },
    ))
}

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
