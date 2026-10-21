// Purpose: 인덱스 include 도달과 같은 서명 선언 그룹의 디스크 수명·정체성 계산.
// Pattern: Scoped Disk Staging.
// Usage: C/C++ 타입·free function 단계가 행을 적재하고 그룹을 연결.
// Related: crates/kodstar-core/src/application/analysis/resolve/cxx/types.rs, crates/kodstar-core/src/application/analysis/resolve/cxx/functions.rs.

pub(super) mod declarations;
pub(super) mod functions;
pub(super) mod types;

use crate::application::analysis::resolve::indexed_paths::normalize;
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::infrastructure::cache::paths::{path_bytes, path_from_bytes};
use rusqlite::{Connection, params};
use std::path::Path;

pub(in crate::application::analysis::resolve) fn reset(
    c: &Connection,
    cancel: &Cancellation,
) -> Result<()> {
    c.execute_batch("CREATE TEMP VIEW IF NOT EXISTS cxx_conditions AS
        SELECT s.id,CASE WHEN s.condition=g.type_name THEN ''
          WHEN substr(s.condition,-length(g.type_name)-4)=' && '||g.type_name THEN substr(s.condition,1,length(s.condition)-length(g.type_name)-4)
          ELSE COALESCE(s.condition,'') END AS effective
        FROM symbols s LEFT JOIN bindings g ON g.file_id=s.file_id AND g.kind='cxx_header_guard'
        WHERE s.language IN ('c','cpp');")?;
    c.execute_batch("CREATE TEMP TABLE IF NOT EXISTS cxx_includes(source INTEGER,target INTEGER,PRIMARY KEY(source,target));
        CREATE TEMP TABLE IF NOT EXISTS cxx_reach(source INTEGER,target INTEGER,PRIMARY KEY(source,target));
        CREATE TEMP TABLE IF NOT EXISTS cxx_reach_ready(id INTEGER PRIMARY KEY);
        CREATE TEMP TABLE IF NOT EXISTS cxx_rows(id INTEGER PRIMARY KEY,file INTEGER,identity TEXT,definition INTEGER);
        CREATE INDEX IF NOT EXISTS cxx_row_names ON cxx_rows(identity,file);
        CREATE TEMP TABLE IF NOT EXISTS cxx_assigned(id INTEGER PRIMARY KEY);
        CREATE TEMP TABLE IF NOT EXISTS cxx_component(id INTEGER PRIMARY KEY);
        CREATE TEMP TABLE IF NOT EXISTS cxx_conflicts(id INTEGER PRIMARY KEY,component INTEGER);
        CREATE INDEX IF NOT EXISTS cxx_conflict_groups ON cxx_conflicts(component);
        DELETE FROM cxx_includes; DELETE FROM cxx_reach; DELETE FROM cxx_reach_ready; DELETE FROM cxx_conflicts;")?;
    let mut statement = c.prepare(
        "SELECT b.file_id,f.path,b.module_name
        FROM bindings b
        JOIN files f ON f.id=b.file_id
        WHERE b.kind='include'
        AND f.language IN ('c','cpp')
        AND NOT EXISTS(SELECT 1
        FROM facts u
        WHERE u.file_id=b.file_id
        AND u.start_byte=b.start_byte
        AND u.relation='includes'
        AND u.condition IS NOT NULL)
        ORDER BY b.id",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let file: i64 = row.get(0)?;
        let raw: Vec<u8> = row.get(1)?;
        let path = path_from_bytes(&raw)?;
        let target: String = row.get(2)?;
        let Some(raw) = target.strip_prefix("@include:") else {
            continue;
        };
        let Some(target) = normalize(path.parent().unwrap_or_else(|| Path::new("")), raw) else {
            continue;
        };
        c.execute("INSERT OR IGNORE INTO cxx_includes SELECT ?1,id FROM files WHERE path=?2 AND language IN ('c','cpp') AND status='analyzed'",params![file,path_bytes(&target)])?;
    }
    Ok(())
}

pub(in crate::application::analysis::resolve) fn stage(c: &Connection) -> Result<()> {
    c.execute_batch("DELETE FROM cxx_rows; DELETE FROM cxx_assigned; DELETE FROM cxx_component;")?;
    Ok(())
}

pub(in crate::application::analysis::resolve) fn merge(
    c: &Connection,
    cancel: &Cancellation,
) -> Result<()> {
    let mut sources=c.prepare("SELECT DISTINCT file FROM cxx_rows WHERE file NOT IN (SELECT id FROM cxx_reach_ready) ORDER BY file")?;
    let mut sources = sources.query([])?;
    while let Some(row) = sources.next()? {
        cancel.check()?;
        let source: i64 = row.get(0)?;
        c.execute("WITH RECURSIVE reach(id) AS (SELECT ?1 UNION SELECT i.target FROM reach r JOIN cxx_includes i ON i.source=r.id)
            INSERT OR IGNORE INTO cxx_reach SELECT ?1,id FROM reach",[source])?;
        c.execute("INSERT INTO cxx_reach_ready VALUES(?1)", [source])?;
    }
    let mut roots = c.prepare("SELECT id FROM cxx_rows ORDER BY identity,id")?;
    let mut roots = roots.query([])?;
    while let Some(row) = roots.next()? {
        cancel.check()?;
        let root: i64 = row.get(0)?;
        let assigned: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM cxx_assigned WHERE id=?1)",
            [root],
            |r| r.get(0),
        )?;
        if assigned {
            continue;
        }
        c.execute_batch("DELETE FROM cxx_component;")?;
        c.execute("WITH RECURSIVE connected(id,file,identity) AS (
            SELECT id,file,identity FROM cxx_rows WHERE id=?1
            UNION SELECT b.id,b.file,b.identity FROM connected a JOIN cxx_rows b ON b.identity=a.identity
            WHERE b.file=a.file OR EXISTS(SELECT 1 FROM cxx_reach r WHERE (r.source=a.file AND r.target=b.file) OR (r.source=b.file AND r.target=a.file))
          ) INSERT INTO cxx_component SELECT id FROM connected",[root])?;
        c.execute("INSERT INTO cxx_assigned SELECT id FROM cxx_component", [])?;
        let definitions: u64 = c.query_row(
            "SELECT SUM(r.definition) FROM cxx_rows r JOIN cxx_component m ON m.id=r.id",
            [],
            |r| r.get(0),
        )?;
        if definitions > 1 {
            c.execute(
                "INSERT INTO cxx_conflicts SELECT id,?1 FROM cxx_component",
                [root],
            )?;
            continue;
        }
        let (identity,anchor):(String,Vec<u8>)=c.query_row("SELECT r.identity,f.path FROM cxx_rows r JOIN cxx_component m ON m.id=r.id JOIN files f ON f.id=r.file ORDER BY r.definition,f.path,r.id LIMIT 1",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
        let mut hash = blake3::Hasher::new();
        hash.update(b"cxx-include-component");
        for part in [identity.as_bytes(), anchor.as_slice()] {
            hash.update(&(part.len() as u64).to_le_bytes());
            hash.update(part);
        }
        c.execute(
            "UPDATE symbols SET logical_key=?1 WHERE id IN (SELECT id FROM cxx_component)",
            [hash.finalize().to_hex().to_string()],
        )?;
    }
    Ok(())
}

impl crate::application::analysis::resolve::Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn cxx_include_condition(
        &mut self,
        context: &crate::application::analysis::resolve::FileContext,
        scope: u32,
        module: &str,
    ) -> Result<()> {
        let conditional:bool=self.connection.query_row("SELECT
            EXISTS(SELECT 1 FROM bindings b JOIN facts u ON u.file_id=b.file_id AND u.start_byte=b.start_byte AND u.relation='includes'
              WHERE b.file_id=?1 AND b.scope_id=?2 AND b.module_name=?3 AND b.kind='include' AND u.condition IS NOT NULL)
            AND NOT EXISTS(SELECT 1 FROM bindings b JOIN facts u ON u.file_id=b.file_id AND u.start_byte=b.start_byte AND u.relation='includes'
              WHERE b.file_id=?1 AND b.scope_id=?2 AND b.module_name=?3 AND b.kind='include' AND u.condition IS NULL)",params![context.id,scope,module],|r|r.get(0))?;
        if conditional {
            self.candidate_only = true;
            self.reason = Some("include has an unevaluated preprocessing condition");
        }
        Ok(())
    }
    pub(in crate::application::analysis::resolve) fn cxx_included_modules(
        &self,
        depth: usize,
    ) -> Result<()> {
        self.connection.execute(
            "WITH RECURSIVE included(id) AS (
            SELECT id FROM resolution_modules WHERE depth=?1
            UNION SELECT i.target FROM included r JOIN cxx_includes i ON i.source=r.id
          ) INSERT OR IGNORE INTO resolution_modules SELECT ?1,id FROM included",
            [depth],
        )?;
        Ok(())
    }
    pub(in crate::application::analysis::resolve) fn cxx_targets(&self) -> Result<()> {
        if !matches!(
            self.language,
            crate::domain::languages::Language::C | crate::domain::languages::Language::Cpp
        ) {
            return Ok(());
        }
        self.connection.execute("INSERT OR IGNORE INTO resolution_candidates
            SELECT b.id FROM cxx_conflicts a JOIN resolution_candidates c ON c.id=a.id JOIN cxx_conflicts b ON b.component=a.component",[])?;
        Ok(())
    }
}
