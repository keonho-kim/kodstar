// Purpose: 사실 재사용과 변경 파일 해석을 허용하는 보수적인 조건 판정.
// Pattern: Specification.
// Usage: writer가 환경이 같은 본문 후보에만 검사하고 설정·삭제는 분석이 반영.
// Related: crates/kodstar-core/src/application/analysis/mod.rs, crates/kodstar-core/src/domain/facts/fingerprint.rs.

use crate::domain::diagnostics::Result;
use crate::infrastructure::index::FileRecord;
use crate::infrastructure::index::invalidation;
use rusqlite::Connection;

#[derive(Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd)]
pub(super) enum Resolution {
    #[default]
    None,
    Selected,
    All,
}

#[derive(Default)]
pub(super) struct ReusePolicy {
    rust_functions: Option<bool>,
    go_only: Option<bool>,
}

impl ReusePolicy {
    pub fn bodies(&mut self, connection: &Connection, language: &str) -> Result<bool> {
        if language == "go" {
            return Ok(true);
        }
        if language != "rust" {
            return Ok(false);
        }
        let eligible = match self.rust_functions {
            Some(eligible) => eligible,
            None => {
                let eligible = invalidation::rust_function_environment(connection)?;
                self.rust_functions = Some(eligible);
                eligible
            }
        };
        Ok(eligible)
    }

    pub fn declarations(&mut self, connection: &Connection, language: &str) -> Result<bool> {
        if language != "go" {
            return Ok(false);
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
        && !invalidation::package_functions(connection, id)?
    {
        return Ok(false);
    }
    invalidation::capture(connection, record)?;
    Ok(true)
}
