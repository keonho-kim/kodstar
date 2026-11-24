// Purpose: 한 수집 transaction의 이전 파일 상태와 관찰 행 SQL 재사용.
// Pattern: Prepared Statements.
// Usage: capture가 순회 동안 소유하고 commit 전에 해제.
// Related: crates/kodstar-core/src/application/history/capture.rs.

use rusqlite::{Connection, OptionalExtension, Params, Result, Statement, params};

pub(super) struct Previous {
    pub blob: i64,
    pub size: u64,
    pub modified: String,
    pub changed: String,
    pub mode: u32,
    pub kind: String,
}

pub(super) struct Catalog<'conn> {
    previous: Statement<'conn>,
    observed: Statement<'conn>,
}

impl<'conn> Catalog<'conn> {
    pub fn new(c: &'conn Connection) -> Result<Self> {
        Ok(Self {
            previous: c.prepare(
                "SELECT blob,size,modified,changed,mode,kind FROM working WHERE path=?1",
            )?,
            observed: c.prepare("INSERT INTO observed VALUES(?1,?2,?3,?4,?5,?6,?7)")?,
        })
    }
    pub fn previous(&mut self, path: &[u8]) -> Result<Option<Previous>> {
        let row = self
            .previous
            .query_row(params![path], |r| {
                Ok(Previous {
                    blob: r.get(0)?,
                    size: r.get(1)?,
                    modified: r.get(2)?,
                    changed: r.get(3)?,
                    mode: r.get(4)?,
                    kind: r.get(5)?,
                })
            })
            .optional();
        self.previous.clear_bindings();
        row
    }
    pub fn observe(&mut self, params: impl Params) -> Result<()> {
        let result = self.observed.execute(params).map(|_| ());
        self.observed.clear_bindings();
        result
    }
}
