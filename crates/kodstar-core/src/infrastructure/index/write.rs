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
    pub modified_ns: String,
    pub changed_ns: String,
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
            + self.modified_ns.capacity()
            + self.changed_ns.capacity()
    }
    pub(crate) fn refresh_metadata(&self, connection: &Connection, generation: i64) -> Result<()> {
        connection.execute("UPDATE files SET content_hash=?1,size=?2,modified_ns=?3,changed_ns=?4,seen_generation=?5 WHERE path=?6", params![self.content_hash,self.size,self.modified_ns,self.changed_ns,generation,path_bytes(&self.path)])?;
        Ok(())
    }

    pub(crate) fn refresh_unchanged(
        &self,
        connection: &Connection,
        generation: i64,
        span: SourceSpan,
    ) -> Result<()> {
        self.refresh_metadata(connection, generation)?;
        let id: i64 = connection.query_row(
            "SELECT id FROM files WHERE path=?1",
            [path_bytes(&self.path)],
            |r| r.get(0),
        )?;
        connection.execute(
            "UPDATE scopes SET start_byte=?1,end_byte=?2 WHERE file_id=?3 AND local_id=0",
            params![span.start_byte, span.end_byte, id],
        )?;
        connection.execute("UPDATE symbols SET start_byte=?1,end_byte=?2,line=?3,column_byte=?4,end_line=?5,end_column_byte=?6 WHERE file_id=?7 AND local_id=0",
            params![span.start_byte,span.end_byte,span.line,span.column,span.end_line,span.end_column,id])?;
        Ok(())
    }

    pub(crate) fn persist(&self, connection: &Connection, generation: i64) -> Result<i64> {
        connection.execute("INSERT INTO files(path,display_path,language,module,package,content_hash,size,modified_ns,changed_ns,seen_generation,status,path_module,path_package) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?4,?5) ON CONFLICT(path) DO UPDATE SET display_path=excluded.display_path,language=excluded.language,module=excluded.module,path_module=excluded.path_module,path_package=excluded.path_package,package=excluded.package,content_hash=excluded.content_hash,size=excluded.size,modified_ns=excluded.modified_ns,changed_ns=excluded.changed_ns,seen_generation=excluded.seen_generation,status=excluded.status",
            params![path_bytes(&self.path), self.path.to_string_lossy(), self.language, self.module, self.package, self.content_hash,
                self.size, self.modified_ns, self.changed_ns, generation, self.status])?;
        Ok(connection.query_row(
            "SELECT id FROM files WHERE path=?1",
            [path_bytes(&self.path)],
            |row| row.get(0),
        )?)
    }
}

pub(crate) fn clear_file(connection: &Connection, id: i64) -> Result<()> {
    connection.execute(
        "UPDATE files SET module=path_module,declared_module='',facts_hash='',environment_hash='' WHERE id=?1",
        [id],
    )?;
    connection.execute("DELETE FROM facts WHERE file_id=?1", [id])?;
    connection.execute("DELETE FROM bindings WHERE file_id=?1", [id])?;
    connection.execute("DELETE FROM symbols WHERE file_id=?1", [id])?;
    connection.execute("DELETE FROM scopes WHERE file_id=?1", [id])?;
    connection.execute("DELETE FROM diagnostics WHERE file_id=?1", [id])?;
    Ok(())
}
