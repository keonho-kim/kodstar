// Purpose: 사전·관계의 고정 행을 SQLite 값 복제 없이 노출.
// Pattern: Adapter.
// Usage: packed cursor가 현재 블록의 선택된 행을 읽음.
// Related: crates/kodstar-core/src/infrastructure/index/packed/cursor.rs, crates/kodstar-core/src/infrastructure/index/relations/row.rs.

use super::super::relations::Relation;
use super::Filter;
use rusqlite::types::{Value, ValueRef};

#[derive(Default)]
pub(in crate::infrastructure::index) enum Rows {
    #[default]
    Empty,
    Keys {
        kind: String,
        keys: Vec<(i64, String)>,
    },
    Relations(Vec<Relation>),
    Lookups {
        group: i64,
        file: i64,
        page: i64,
        rows: Vec<super::super::dependencies::pages::Row>,
    },
}

impl Rows {
    pub fn len(&self) -> usize {
        match self {
            Self::Empty => 0,
            Self::Keys { keys, .. } => keys.len(),
            Self::Relations(rows) => rows.len(),
            Self::Lookups { rows, .. } => rows.len(),
        }
    }

    pub fn value(&self, at: usize, column: usize) -> ValueRef<'_> {
        match self {
            Self::Empty => ValueRef::Null,
            Self::Keys { kind, keys } => match column {
                0 => ValueRef::Integer(keys[at].0),
                1 => ValueRef::Text(kind.as_bytes()),
                2 => ValueRef::Text(keys[at].1.as_bytes()),
                _ => ValueRef::Null,
            },
            Self::Relations(rows) => rows[at].value(column),
            Self::Lookups {
                group, file, rows, ..
            } => super::super::dependencies::pages::value_at(*group, *file, &rows[at], column),
        }
    }

    pub fn rowid(&self, at: usize) -> rusqlite::Result<i64> {
        if let Self::Lookups { page, rows, .. } = self {
            return super::super::dependencies::pages::identity(*page, &rows[at]);
        }
        match self.value(at, 0) {
            ValueRef::Integer(id) => Ok(id),
            _ => Err(super::codec::invalid()),
        }
    }
}

pub(in crate::infrastructure::index) fn matches(value: ValueRef<'_>, expected: &Value) -> bool {
    match (value, expected) {
        (_, Value::Null) => false,
        (ValueRef::Integer(a), Value::Integer(b)) => a == *b,
        (ValueRef::Text(a), Value::Text(b)) => a == b.as_bytes(),
        // SQLite rechecks affinity and coercion for every forwarded constraint.
        _ => true,
    }
}

pub(in crate::infrastructure::index) fn key_matches(
    filters: &[Filter],
    id: i64,
    kind: &str,
    key: &str,
) -> bool {
    filters.iter().all(|(column, value)| {
        matches(
            match column {
                0 => ValueRef::Integer(id),
                1 => ValueRef::Text(kind.as_bytes()),
                2 => ValueRef::Text(key.as_bytes()),
                _ => ValueRef::Null,
            },
            value,
        )
    })
}
