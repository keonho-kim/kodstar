// Purpose: 체크포인트 파일 구성을 분석 입력으로 순회.
// Pattern: Input Adapter.
// Usage: analysis discovery와 명시 설정 읽기가 저장된 소스를 선택.
// Related: crates/kodstar-core/src/infrastructure/history/inputs/entry.rs.

mod entry;
pub(crate) use super::content::chunk_read::ReadWorkspace;
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
    languages::Language,
};
use crate::infrastructure::{
    cache::paths::{path_bytes, path_from_bytes},
    formats, languages,
};
pub(crate) use entry::Entry;
use rusqlite::OptionalExtension;
use std::path::Path;
use std::rc::Rc;

pub(crate) fn walk(
    connection: Rc<rusqlite::Connection>,
    changed_only: bool,
    excludes: &[String],
    overrides: &[(globset::GlobMatcher, Language)],
    cancel: &Cancellation,
    emit: &mut dyn FnMut(Entry) -> Result<()>,
) -> Result<()> {
    let excluded = excludes
        .iter()
        .map(|s| globset::Glob::new(s).map(|g| g.compile_matcher()))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::caused_by("compile snapshot exclusions", e))?;
    let source = if changed_only {
        "snapshot_files f JOIN (SELECT path FROM snapshot_changes UNION SELECT path FROM analysis_extra_inputs) delta ON f.path=delta.path"
    } else {
        "snapshot_files f"
    };
    let mut statement = connection.prepare(&format!("SELECT f.path,f.blob,b.size,b.hash FROM {source} JOIN blobs b ON b.id=f.blob WHERE f.kind='file' ORDER BY f.path"))?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let relative = path_from_bytes(&row.get::<_, Vec<u8>>(0)?)?;
        super::paths::relative(&relative)?;
        if relative
            .ancestors()
            .filter(|p| !p.as_os_str().is_empty())
            .any(|p| excluded.iter().any(|g| g.is_match(p)))
        {
            continue;
        }
        let language = overrides
            .iter()
            .find(|(pattern, _)| pattern.is_match(&relative))
            .map(|(_, language)| *language);
        let has_override = language.is_some();
        let language = language.or_else(|| languages::detect(&relative, &[]));
        let detect_prefix = !has_override
            && (relative.extension().is_some_and(|v| v == "h")
                || relative.extension().is_none() && language.is_none());
        emit(Entry {
            auxiliary: formats::is_auxiliary(&relative),
            relative,
            connection: Rc::clone(&connection),
            blob: row.get(1)?,
            size: row.get(2)?,
            hash: row.get(3)?,
            language,
            detect_prefix,
        })?;
    }
    Ok(())
}

pub(crate) struct Reader<'conn> {
    connection: &'conn Rc<rusqlite::Connection>,
    statement: rusqlite::CachedStatement<'conn>,
}

impl<'conn> Reader<'conn> {
    pub(crate) fn new(connection: &'conn Rc<rusqlite::Connection>) -> Result<Self> {
        Ok(Self { connection, statement: connection.prepare_cached("SELECT f.blob,b.size,b.hash FROM snapshot_files f JOIN blobs b ON b.id=f.blob WHERE f.path=?1 AND f.kind='file'")? })
    }

    pub(crate) fn get(&mut self, path: &Path) -> Result<Entry> {
        super::paths::relative(path)?;
        let row: Option<(i64, u64, [u8; 32])> = self
            .statement
            .query_row([path_bytes(path)], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?;
        let (blob, size, hash) =
            row.ok_or_else(|| Error::not_found("configuration is absent from the checkpoint"))?;
        Ok(Entry {
            relative: path.to_path_buf(),
            connection: Rc::clone(self.connection),
            blob,
            size,
            hash,
            language: None,
            auxiliary: true,
            detect_prefix: false,
        })
    }
}
