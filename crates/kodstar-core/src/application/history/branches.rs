// Purpose: 브랜치 참조 생성·삭제와 보호된 전환.
// Pattern: Use Case.
// Usage: history handler가 사전 보존 후 호출.
// Related: crates/kodstar-core/src/infrastructure/history/checkout/mod.rs.

use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
    history::HistoryRequest,
};
use crate::infrastructure::history::{Repository, checkout};
use rusqlite::{OptionalExtension, params};

pub(super) fn run(
    repository: &Repository,
    request: &HistoryRequest,
    head: i64,
    cancel: &Cancellation,
) -> Result<()> {
    let connection = &repository.connection;
    match request {
        HistoryRequest::CreateBranch { name } => {
            let exists: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM branches WHERE name=?1)",
                [name],
                |r| r.get(0),
            )?;
            if exists {
                return Err(Error::argument("branch already exists"));
            }
            connection.execute("INSERT INTO branches VALUES(?1,?2)", params![name, head])?;
        }
        HistoryRequest::SwitchBranch { name } => {
            let target: Option<i64> = connection
                .query_row(
                    "SELECT checkpoint FROM branches WHERE name=?1",
                    [name],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            let target = target.ok_or_else(|| Error::not_found("branch not found"))?;
            checkout::start(repository, target, Some(name), None, cancel)?;
        }
        HistoryRequest::DeleteBranch { name } => {
            let current: String =
                connection.query_row("SELECT current_branch FROM repository", [], |r| r.get(0))?;
            if current == *name {
                return Err(Error::argument("cannot delete the current branch"));
            }
            if connection.execute("DELETE FROM branches WHERE name=?1", [name])? == 0 {
                return Err(Error::not_found("branch not found"));
            }
        }
        _ => return Err(Error::argument("expected a branch mutation")),
    }
    Ok(())
}
