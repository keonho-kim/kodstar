// Purpose: 파일 metadata의 SQL 저장·정리.
// Pattern: Storage Adapter.
// Usage: 분석 pipeline이 쓰기 transaction 안에서 호출.
// Related: crates/kodstar-core/src/application/analysis/pipeline.rs, crates/kodstar-core/src/infrastructure/index/schema.sql.

use rusqlite::{Connection, params};

use crate::domain::diagnostics::Result;
use crate::domain::source::SourceSpan;
use crate::infrastructure::cache::paths::path_bytes;

#[derive(Clone, Debug)]
pub(crate) struct FileRecord {
    pub path: std::path::PathBuf,
    pub language: String,
    pub module: String,
    pub package: String,
    pub content_hash: String,
    pub size: u64,
    pub status: &'static str,
}

impl FileRecord {
    pub(crate) fn owned_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.path.capacity()
            + self.language.capacity()
            + self.module.capacity()
            + self.package.capacity()
            + self.content_hash.capacity()
    }
    pub(crate) fn refresh_metadata(&self, connection: &Connection) -> Result<()> {
        connection.execute(
            "UPDATE files_rows SET content_hash=?1,size=?2 WHERE path=?3",
            params![
                &super::hash_bytes(&self.content_hash)?[..],
                self.size,
                path_bytes(&self.path)
            ],
        )?;
        Ok(())
    }

    pub(crate) fn refresh_unchanged(
        &self,
        connection: &Connection,
        span: SourceSpan,
        scope_ends: &[(u32, usize)],
    ) -> Result<()> {
        self.refresh_metadata(connection)?;
        let id: i64 = connection.query_row(
            "SELECT id FROM files WHERE path=?1",
            [path_bytes(&self.path)],
            |r| r.get(0),
        )?;
        connection.execute(
            "UPDATE scopes SET start_byte=?1,end_byte=?2 WHERE file_id=?3 AND local_id=0",
            params![span.start_byte, span.end_byte, id],
        )?;
        connection.execute("UPDATE symbols_rows SET start_byte=?1,end_byte=?2,line=?3,column_byte=?4,end_line=?5,end_column_byte=?6 WHERE file_id=?7 AND local_id=0",
            params![span.start_byte,span.end_byte,span.line,span.column,span.end_line,span.end_column,id])?;
        let mut update =
            connection.prepare("UPDATE scopes SET end_byte=?1 WHERE file_id=?2 AND local_id=?3")?;
        for (local, end) in scope_ends {
            if update.execute(params![end, id, local])? != 1 {
                return Err(crate::domain::diagnostics::Error::operation(
                    "reused scope is missing",
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn persist(&self, connection: &super::cached::Sql<'_>) -> Result<i64> {
        connection.cached_execute("INSERT INTO files(path,language,module,package,content_hash,size,status,path_module,path_package) VALUES(?1,?2,?3,?4,?5,?6,?7,?3,?4)",
            params![path_bytes(&self.path), self.language, self.module, self.package, &super::hash_bytes(&self.content_hash)?[..], self.size, self.status])?;
        Ok(connection.cached_row(
            "SELECT id FROM files WHERE path=?1",
            [path_bytes(&self.path)],
            |row| row.get(0),
        )?)
    }
}

pub(crate) fn clear_file(connection: &super::cached::Sql<'_>, id: i64) -> Result<()> {
    connection.cached_execute("DELETE FROM lookup_rows WHERE file_id=?1", [id])?;
    connection.cached_execute("DELETE FROM relation_blocks WHERE file_id=?1", [id])?;
    connection.cached_execute(
        "UPDATE files SET module=path_module,declared_module='',facts_hash=X'',environment_hash=X'' WHERE id=?1",
        [id],
    )?;
    connection.cached_execute("DELETE FROM diagnostics WHERE file_id=?1", [id])?;
    // Every definition, binding and fact has a scope FK. New and inert files
    // therefore need no statement planning for their empty cascade trees.
    let scoped: bool = connection.cached_row(
        "SELECT EXISTS(SELECT 1 FROM scopes WHERE file_id=?1)",
        [id],
        |r| r.get(0),
    )?;
    if !scoped {
        return Ok(());
    }
    super::relations::remove_targets(connection, Some(id))?;
    connection.cached_execute("DELETE FROM facts WHERE file_id=?1", [id])?;
    connection.cached_execute("DELETE FROM bindings WHERE file_id=?1", [id])?;
    connection.cached_execute("DELETE FROM symbols WHERE file_id=?1", [id])?;
    connection.cached_execute("DELETE FROM scopes WHERE file_id=?1", [id])?;
    Ok(())
}
