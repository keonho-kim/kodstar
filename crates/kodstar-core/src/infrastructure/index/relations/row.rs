// Purpose: 관계의 고정 필드와 SQLite 경계의 값 변환.
// Pattern: Typed Row.
// Usage: 관계 codec·writer·packed cursor가 같은 행 계약을 사용.
// Related: crates/kodstar-core/src/infrastructure/index/relations/codec.rs, crates/kodstar-core/src/infrastructure/index/packed/rows.rs.

use super::super::packed::codec::invalid;
use crate::domain::relations::Resolution;
use rusqlite::{
    Result,
    types::{Value, ValueRef},
};

#[derive(Debug, PartialEq)]
pub(crate) struct Relation {
    pub id: i64,
    pub fact: i64,
    pub source: i64,
    pub target: Option<i64>,
    pub external: Option<String>,
    pub state: Resolution,
    pub rule: i64,
    pub reason: Option<i64>,
}

impl Relation {
    pub fn from_values(mut values: Vec<Value>) -> Result<Self> {
        if values.len() != 8 {
            return Err(invalid());
        }
        let id = |column, nullable| match values[column] {
            Value::Null if nullable => Ok(None),
            Value::Integer(n) if n > 0 => Ok(Some(n)),
            _ => Err(invalid()),
        };
        let relation = Self {
            id: id(0, false)?.ok_or_else(invalid)?,
            fact: id(1, false)?.ok_or_else(invalid)?,
            source: id(2, false)?.ok_or_else(invalid)?,
            target: id(3, true)?,
            rule: id(6, false)?.ok_or_else(invalid)?,
            reason: id(7, true)?,
            state: match &values[5] {
                Value::Text(s) => Self::state(s)?,
                _ => return Err(invalid()),
            },
            external: match std::mem::replace(&mut values[4], Value::Null) {
                Value::Text(s) => Some(s),
                Value::Null => None,
                _ => return Err(invalid()),
            },
        };
        relation.validate()?;
        Ok(relation)
    }

    pub fn validate(&self) -> Result<()> {
        if self.id <= 0
            || self.fact <= 0
            || self.source <= 0
            || self.rule <= 0
            || self.target.is_some_and(|n| n <= 0)
            || self.reason.is_some_and(|n| n <= 0)
            || self.target.is_some() && self.external.is_some()
            || self.state == Resolution::Resolved
                && self.target.is_none()
                && self.external.is_none()
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub fn state(value: &str) -> Result<Resolution> {
        match value {
            "resolved" => Ok(Resolution::Resolved),
            "candidate" => Ok(Resolution::Candidate),
            "ambiguous" => Ok(Resolution::Ambiguous),
            "unresolved" => Ok(Resolution::Unresolved),
            _ => Err(invalid()),
        }
    }

    pub fn value(&self, column: usize) -> ValueRef<'_> {
        match column {
            0 => ValueRef::Integer(self.id),
            1 => ValueRef::Integer(self.fact),
            2 => ValueRef::Integer(self.source),
            3 => self.target.map_or(ValueRef::Null, ValueRef::Integer),
            4 => self
                .external
                .as_ref()
                .map_or(ValueRef::Null, |s| ValueRef::Text(s.as_bytes())),
            5 => ValueRef::Text(self.state.as_str().as_bytes()),
            6 => ValueRef::Integer(self.rule),
            7 => self.reason.map_or(ValueRef::Null, ValueRef::Integer),
            _ => ValueRef::Null,
        }
    }

    pub fn routes(&self) -> [Option<i64>; 5] {
        [
            Some(self.id),
            Some(self.fact),
            self.target,
            Some(self.rule),
            self.reason,
        ]
    }

    pub fn owned_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.external.as_ref().map_or(0, String::capacity)
    }
}
