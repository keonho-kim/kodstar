// Purpose: 한 native 파일에서 반복되는 이름·USR·경로의 소유 문자열 사전.
// Pattern: File-scoped Interner.
// Usage: fact 수집기가 key를 저장하고 Facts가 빌린 문자열 view를 복원.
// Related: crates/kodstar-clang/src/facts.rs, crates/kodstar-clang/src/callbacks.rs.

use std::collections::hash_map::RandomState;
use string_interner::{StringInterner, backend::BucketBackend, symbol::SymbolU32};

pub(super) type Key = SymbolU32;

pub(super) struct Texts(StringInterner<BucketBackend<Key>, RandomState>);

impl Default for Texts {
    fn default() -> Self {
        Self(StringInterner::with_hasher(RandomState::new()))
    }
}

impl Texts {
    pub fn intern(&mut self, text: &str) -> Key {
        self.0.get_or_intern(text)
    }

    pub fn resolve(&self, key: Key) -> &str {
        self.0
            .resolve(key)
            .expect("native text key belongs to this file")
    }
}
