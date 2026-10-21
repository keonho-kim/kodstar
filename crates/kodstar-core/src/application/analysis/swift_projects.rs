// Purpose: 인덱스의 정적 SwiftPM target 경로로 파일의 모듈 소속 계산.
// Pattern: Simple Module.
// Usage: 분석 transaction이 이름 해석과 논리 정체성 계산 전에 실행.
// Related: crates/kodstar-core/src/infrastructure/parse/swift/packages.rs, crates/kodstar-core/src/application/analysis/resolve/swift/mod.rs.

use super::resolve::indexed_paths::normalize;
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::infrastructure::cache::paths::{cache_key, path_bytes, path_from_bytes};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

pub(super) fn run(c: &Connection, root: &Path, cancel: &Cancellation) -> Result<()> {
    c.execute_batch("CREATE TEMP TABLE IF NOT EXISTS swift_targets(id INTEGER PRIMARY KEY,manifest INTEGER,name TEXT,path BLOB,explicit_sources INTEGER);
        CREATE INDEX IF NOT EXISTS swift_target_names ON swift_targets(manifest,name);
        CREATE TEMP TABLE IF NOT EXISTS swift_membership(file_id INTEGER PRIMARY KEY,manifest INTEGER,target TEXT);
        DELETE FROM swift_targets; DELETE FROM swift_membership;")?;
    targets(c, cancel)?;
    let mut statement =
        c.prepare("SELECT id,path FROM files WHERE language='swift' ORDER BY path")?;
    let records =
        statement.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?;
    for record in records {
        cancel.check()?;
        let (file, raw) = record?;
        let path = path_from_bytes(&raw)?;
        let mut manifest = None;
        let mut directory = Path::new("");
        for parent in path.parent().unwrap_or_else(|| Path::new("")).ancestors() {
            let found: Option<i64> = c
                .query_row(
                    "SELECT id FROM files WHERE path=?1 AND language='swift' AND status='analyzed'",
                    [path_bytes(&parent.join("Package.swift"))],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(found) = found {
                manifest = Some(found);
                directory = parent;
                break;
            }
        }
        let (module, target) = if let Some(manifest) = manifest {
            membership(c, manifest, &path, cancel)?
        } else {
            (
                path.parent()
                    .unwrap_or_else(|| Path::new(""))
                    .to_string_lossy()
                    .into_owned(),
                None,
            )
        };
        c.execute(
            "UPDATE files SET package=?1,module=?2 WHERE id=?3",
            params![cache_key(&root.join(directory)), module, file],
        )?;
        c.execute(
            "INSERT INTO swift_membership VALUES(?1,?2,?3)",
            params![file, manifest, target],
        )?;
    }
    Ok(())
}

fn targets(c: &Connection, cancel: &Cancellation) -> Result<()> {
    let mut statement=c.prepare("SELECT b.id,b.file_id,b.name,b.module_name,f.path,
        EXISTS(SELECT 1 FROM bindings s WHERE s.file_id=b.file_id AND s.name=b.name AND s.kind='swift_target_sources')
        FROM bindings b JOIN files f ON f.id=b.file_id WHERE b.kind='swift_target'
        AND EXISTS(SELECT 1 FROM bindings i WHERE i.file_id=b.file_id AND i.kind='import' AND i.module_name='PackageDescription')
        AND NOT EXISTS(SELECT 1 FROM symbols s WHERE s.file_id=b.file_id AND s.name='Package')
        AND NOT EXISTS(SELECT 1 FROM bindings d WHERE d.file_id=b.file_id AND d.kind='swift_manifest_dynamic')
        AND NOT EXISTS(SELECT 1 FROM bindings d WHERE d.file_id=b.file_id AND d.name='Package' AND d.kind<>'import')
        AND NOT EXISTS(SELECT 1 FROM bindings d WHERE d.file_id=b.file_id AND d.name=b.name AND d.kind='swift_target_dynamic')
        AND (SELECT COUNT(*) FROM bindings d WHERE d.file_id=b.file_id AND d.name=b.name AND d.kind='swift_target')=1")?;
    let records = statement.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Vec<u8>>(4)?,
            r.get::<_, bool>(5)?,
        ))
    })?;
    for record in records {
        cancel.check()?;
        let (id, file, name, raw_path, raw_manifest, explicit) = record?;
        let manifest = path_from_bytes(&raw_manifest)?;
        let parent = manifest.parent().unwrap_or_else(|| Path::new(""));
        let Some(path) = normalize(parent, &raw_path).filter(|p| p.starts_with(parent)) else {
            continue;
        };
        c.execute(
            "INSERT INTO swift_targets VALUES(?1,?2,?3,?4,?5)",
            params![id, file, name, path_bytes(&path), explicit],
        )?;
    }
    Ok(())
}

fn membership(
    c: &Connection,
    manifest: i64,
    path: &Path,
    cancel: &Cancellation,
) -> Result<(String, Option<String>)> {
    let mut statement = c.prepare(
        "SELECT name,path,explicit_sources FROM swift_targets WHERE manifest=?1 ORDER BY id",
    )?;
    let records = statement.query_map([manifest], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Vec<u8>>(1)?,
            r.get::<_, bool>(2)?,
        ))
    })?;
    let mut selected = None;
    for record in records {
        cancel.check()?;
        let (name, raw, explicit) = record?;
        let target_path = path_from_bytes(&raw)?;
        if !path.starts_with(&target_path) {
            continue;
        }
        let mut includes = !explicit;
        let mut excludes = false;
        let mut filters=c.prepare("SELECT kind,module_name FROM bindings WHERE file_id=?1 AND name=?2 AND kind IN ('swift_target_source','swift_target_exclude') ORDER BY id")?;
        let mut rows = filters.query(params![manifest, name])?;
        while let Some(row) = rows.next()? {
            cancel.check()?;
            let kind: String = row.get(0)?;
            let raw: String = row.get(1)?;
            let Some(prefix) =
                normalize(&target_path, &raw).filter(|p| p.starts_with(&target_path))
            else {
                excludes = true;
                continue;
            };
            if path.starts_with(&prefix) {
                if kind == "swift_target_exclude" {
                    excludes = true;
                } else {
                    includes = true;
                }
            }
        }
        if !includes || excludes {
            continue;
        }
        if selected.is_some() {
            return Ok((
                format!(
                    "@swift-ambiguous:{}",
                    blake3::hash(&path_bytes(path)).to_hex()
                ),
                None,
            ));
        }
        selected = Some(name);
    }
    Ok(selected.map_or_else(
        || {
            (
                format!(
                    "@swift-unassigned:{}",
                    blake3::hash(&path_bytes(path)).to_hex()
                ),
                None,
            )
        },
        |name| (name.clone(), Some(name)),
    ))
}

pub(super) fn identities(c: &Connection, cancel: &Cancellation) -> Result<()> {
    let mut statement=c.prepare("SELECT s.id,s.syntax_key,f.module,f.package FROM symbols s JOIN files f ON f.id=s.file_id WHERE s.language='swift' ORDER BY s.id")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let id: i64 = row.get(0)?;
        let mut hash = blake3::Hasher::new();
        hash.update(b"swift-module-definition");
        for column in [1, 2, 3] {
            let part: String = row.get(column)?;
            hash.update(&(part.len() as u64).to_le_bytes());
            hash.update(part.as_bytes());
        }
        c.execute(
            "UPDATE symbols SET logical_key=?1 WHERE id=?2",
            params![hash.finalize().to_hex().to_string(), id],
        )?;
    }
    Ok(())
}
