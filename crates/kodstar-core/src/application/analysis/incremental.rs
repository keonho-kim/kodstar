// Purpose: 사실 재사용·부분 해석의 안전 조건과 전체 해석 전이 관리.
// Pattern: Specification.
// Usage: writer가 환경이 같은 본문 후보에만 검사하고 설정·삭제는 분석이 반영.
// Related: crates/kodstar-core/src/application/analysis/mod.rs, crates/kodstar-core/src/domain/facts/fingerprint.rs.

use crate::domain::diagnostics::Result;
use crate::infrastructure::index::FileRecord;
use crate::infrastructure::index::invalidation;
use rusqlite::Connection;

pub(super) mod inputs;

#[derive(Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd)]
pub(super) enum Resolution {
    #[default]
    None,
    Selected,
    All,
}

impl Resolution {
    pub fn reset_all(&mut self, connection: &Connection) -> Result<()> {
        if *self != Self::All {
            crate::infrastructure::index::statistics::inputs::finish(connection)?;
            crate::infrastructure::index::relations::clear_all(connection)?;
            *self = Self::All;
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct ReusePolicy {
    go_only: Option<bool>,
    simple_modules: Option<(&'static str, bool)>,
}

impl ReusePolicy {
    pub fn declarations(&mut self, connection: &Connection, language: &str) -> Result<bool> {
        if language != "go" {
            let Some(group) = invalidation::static_modules::group(language) else {
                return Ok(false);
            };
            let (selected, eligible) = match self.simple_modules {
                Some(value) => value,
                None => {
                    let value = (
                        group,
                        invalidation::static_modules::project(connection, language)?,
                    );
                    self.simple_modules = Some(value);
                    value
                }
            };
            return Ok(selected == group && eligible);
        }
        let eligible = match self.go_only {
            Some(eligible) => eligible,
            None => {
                let eligible = invalidation::only_go(connection)?;
                self.go_only = Some(eligible);
                eligible
            }
        };
        Ok(eligible)
    }
}

pub(super) fn scoped_declarations(connection: &Connection, record: &FileRecord) -> Result<bool> {
    if let Some(id) = invalidation::prior_file(connection, record)?
        && !invalidation::safe_file(connection, id)?
    {
        return Ok(false);
    }
    invalidation::capture(connection, record)?;
    Ok(true)
}
