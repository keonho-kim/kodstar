// Purpose: 정적 함수 모듈의 부분 해석 조건과 정규화한 provider 무효화.
// Pattern: Storage Specification.
// Usage: 증분 정책이 변경 전후 환경을 검사하고 기존 lookup 소비를 선택.
// Related: crates/kodstar-core/src/infrastructure/index/invalidation.rs, crates/kodstar-core/src/domain/resolve/lookup_keys.rs.

use crate::domain::{diagnostics::Result, resolve::lookup_keys};
use crate::infrastructure::{cache::paths::path_bytes, index::FileRecord};
use rusqlite::{Connection, params};

const BINDINGS: &str = "'function','parameter','import','import_type','export','export_type',
    'python_local','python_global','python_nonlocal','global','nonlocal'";

pub(crate) fn group(language: &str) -> Option<&'static str> {
    match language {
        "python" => Some("'python'"),
        "javascript" | "typescript" => Some("'javascript','typescript'"),
        "rust" => Some("'rust'"),
        "zig" => Some("'zig'"),
        _ => None,
    }
}

pub(crate) fn project(c: &Connection, language: &str) -> Result<bool> {
    let Some(group) = group(language) else {
        return Ok(false);
    };
    Ok(c.query_row(
        &format!(
            "SELECT
        NOT EXISTS(SELECT 1 FROM files WHERE status='analyzed' AND language NOT IN ({group}))
        AND NOT EXISTS(SELECT 1 FROM symbols s JOIN files f ON f.id=s.file_id
            WHERE f.status='analyzed' AND s.kind NOT IN ('file','function'))
        AND NOT EXISTS(SELECT 1 FROM bindings b JOIN files f ON f.id=b.file_id
            WHERE f.status='analyzed' AND b.kind NOT IN ({BINDINGS}))
        AND NOT EXISTS(SELECT 1 FROM symbol_aliases)
        AND NOT EXISTS(SELECT 1 FROM symbols_rows WHERE resolved_key IS NOT NULL
            OR resolved_qualified_name IS NOT NULL OR resolved_kind IS NOT NULL)
        AND NOT EXISTS(SELECT 1 FROM facts WHERE relation IN
            ('inherits','implements','mixes_in','overrides','dispatch_candidate'))"
        ),
        [],
        |r| r.get(0),
    )?)
}

pub(crate) fn file(c: &Connection, id: i64) -> Result<bool> {
    Ok(c.query_row(
        &format!(
            "SELECT
        NOT EXISTS(SELECT 1 FROM symbols WHERE file_id=?1 AND kind NOT IN ('file','function'))
        AND NOT EXISTS(SELECT 1 FROM bindings WHERE file_id=?1 AND kind NOT IN ({BINDINGS}))
        AND NOT EXISTS(SELECT 1 FROM facts WHERE file_id=?1 AND relation IN
            ('inherits','implements','mixes_in','overrides','dispatch_candidate'))"
        ),
        [id],
        |r| r.get(0),
    )?)
}

pub(crate) fn capture(c: &Connection, record: &FileRecord) -> Result<()> {
    let mut remember = c.prepare(
        "INSERT OR IGNORE INTO pending_lookup_keys
        SELECT id FROM lookup_keys WHERE kind=?1 AND lookup_key=?2",
    )?;
    match record.language.as_str() {
        "python" | "rust" => {
            let mut previous = c.prepare("SELECT package,module FROM files WHERE path=?1")?;
            let mut rows = previous.query([path_bytes(&record.path)])?;
            while let Some(row) = rows.next()? {
                remember.execute(params![
                    "module_provider",
                    lookup_keys::module_provider(
                        &record.language,
                        &row.get::<_, String>(0)?,
                        &row.get::<_, String>(1)?
                    )
                ])?;
            }
            remember.execute(params![
                "module_provider",
                lookup_keys::module_provider(&record.language, &record.package, &record.module)
            ])?;
        }
        "javascript" | "typescript" => {
            let path = lookup_keys::javascript_path(&record.path);
            remember.execute(params![
                "path_provider",
                lookup_keys::path_provider("javascript", &path_bytes(&path))
            ])?;
            if path.file_name().is_some_and(|name| name == "index")
                && let Some(parent) = path.parent()
            {
                remember.execute(params![
                    "path_provider",
                    lookup_keys::path_provider("javascript", &path_bytes(parent))
                ])?;
            }
        }
        "zig" => {
            remember.execute(params![
                "path_provider",
                lookup_keys::path_provider("zig", &path_bytes(&record.path))
            ])?;
        }
        _ => {}
    }
    Ok(())
}
