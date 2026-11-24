// Purpose: Hono 라우트·handler와 정적 mount를 세대 수명의 임시 인덱스로 구성.
// Pattern: Bounded Projection.
// Usage: HTTP resolver 준비 단계가 기존 binding을 순회.
// Related: crates/kodstar-core/src/application/analysis/resolve/http/mod.rs, crates/kodstar-core/src/application/analysis/resolve/http/origin.rs.

use super::super::{Lookup, context, prepared::Prepared};
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
    web::{self, Mount, Route},
};
use crate::infrastructure::index::cached::Sql;
use rusqlite::params;

pub(in crate::application::analysis::resolve) fn prepare<'conn>(
    c: &Sql<'conn>,
    cancel: &Cancellation,
    prepared: &Prepared<'conn>,
) -> Result<()> {
    c.execute_batch("CREATE TEMP TABLE IF NOT EXISTS http_routes(file INTEGER,slot INTEGER,root TEXT,method TEXT,path TEXT,target INTEGER,last INTEGER,PRIMARY KEY(file,slot,target)) WITHOUT ROWID;
        CREATE TEMP TABLE IF NOT EXISTS http_mounts(parent TEXT,prefix TEXT,child TEXT,PRIMARY KEY(parent,prefix,child)) WITHOUT ROWID;
        CREATE TEMP TABLE IF NOT EXISTS http_effective(root TEXT,method TEXT,path TEXT,target INTEGER,last INTEGER,depth INTEGER,PRIMARY KEY(root,method,path,target,last)) WITHOUT ROWID;
        CREATE INDEX IF NOT EXISTS http_method_path ON http_effective(method,path);
        DELETE FROM http_routes; DELETE FROM http_mounts; DELETE FROM http_effective;")?;
    let present: bool = c.cached_row(
        "SELECT EXISTS(SELECT 1 FROM atoms WHERE value='hono_route')",
        [],
        |r| r.get(0),
    )?;
    if !present {
        return Ok(());
    }
    let mut total: usize = 0;
    let mut q = c.cached_prepare("SELECT file_id,scope_id,start_byte,target_local,module_name FROM bindings WHERE kind='hono_route' ORDER BY id")?;
    let mut rows = q.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let (file, scope, position, target, data): (i64, u32, usize, Option<u32>, String) = (
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
        );
        let route: Route = serde_json::from_str(&data)
            .map_err(|e| Error::caused_by("invalid Hono route evidence", e))?;
        let Some(context) = context(c, file, None)? else {
            continue;
        };
        let mut lookup = Lookup::new(c, prepared, &context, None, None, "handles", "");
        let Some(app) = lookup.hono_app(&context, scope, position, &route.app, 0)? else {
            continue;
        };
        let Some(path) = web::join(&app.prefix, &route.path) else {
            continue;
        };
        c.cached_execute("DELETE FROM resolution_candidates;", [])?;
        if let Some(target) = target {
            c.cached_execute("INSERT OR IGNORE INTO resolution_candidates SELECT id FROM symbols WHERE file_id=?1 AND local_id=?2 AND kind IN ('function','method')",params![file,target])?;
        } else if let Some(handler) = route.handler {
            lookup.expression(&context, scope, position, &handler, 0)?;
            c.cached_execute("DELETE FROM resolution_candidates WHERE id NOT IN (SELECT id FROM symbols WHERE kind IN ('function','method'))",[])?;
        }
        total += c.cached_execute("INSERT OR IGNORE INTO http_routes SELECT ?1,?2,?3,?4,?5,id,?6 FROM resolution_candidates",params![file,position,app.root,route.method,path,route.last])?;
        if total > 16384 || path.len() > 65536 {
            return Err(Error::new(
                crate::domain::diagnostics::ErrorKind::Limit,
                "HTTP registration count or path limit exceeded",
            ));
        }
        c.cached_execute("DELETE FROM resolution_candidates", [])?;
        c.cached_execute("DELETE FROM resolution_externals", [])?;
    }
    drop(rows);
    drop(q);
    let mut q = c.cached_prepare("SELECT file_id,scope_id,start_byte,module_name FROM bindings WHERE kind='hono_mount' ORDER BY id")?;
    let mut rows = q.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let (file, scope, position, data): (i64, u32, usize, String) =
            (row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?);
        let mount: Mount = serde_json::from_str(&data)
            .map_err(|e| Error::caused_by("invalid Hono mount evidence", e))?;
        let Some(context) = context(c, file, None)? else {
            continue;
        };
        let mut lookup = Lookup::new(c, prepared, &context, None, None, "handles", "");
        if let (Some(parent), Some(child)) = (
            lookup.hono_app(&context, scope, position, &mount.parent, 0)?,
            lookup.hono_app(&context, scope, position, &mount.child, 0)?,
        ) && let Some(prefix) = web::join(&parent.prefix, &mount.path)
        {
            c.cached_execute(
                "INSERT OR IGNORE INTO http_mounts VALUES(?1,?2,?3)",
                params![parent.root, prefix, child.root],
            )?;
        }
    }
    c.cached_execute("INSERT OR IGNORE INTO http_effective SELECT root,method,path,target,last,0 FROM http_routes",[])?;
    for depth in 0..16 {
        cancel.check()?;
        let mut q=c.cached_prepare("SELECT m.parent,m.prefix,r.method,r.path,r.target,r.last FROM http_mounts m JOIN http_effective r ON r.root=m.child WHERE r.depth=?1")?;
        let mut rows = q.query([depth])?;
        let mut inserted = 0;
        while let Some(row) = rows.next()? {
            cancel.check()?;
            let prefix: String = row.get(1)?;
            let path: String = row.get(3)?;
            if let Some(path) = web::join(&prefix, &path) {
                let added = c.cached_execute(
                    "INSERT OR IGNORE INTO http_effective VALUES(?1,?2,?3,?4,?5,?6)",
                    params![
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(2)?,
                        path,
                        row.get::<_, i64>(4)?,
                        row.get::<_, bool>(5)?,
                        depth + 1
                    ],
                )?;
                inserted += added;
                total += added;
                if total > 16384 || path.len() > 65536 {
                    return Err(Error::new(
                        crate::domain::diagnostics::ErrorKind::Limit,
                        "HTTP route projection limit exceeded",
                    ));
                }
            }
        }
        let count: usize = c.cached_row("SELECT COUNT(*) FROM http_effective", [], |r| r.get(0))?;
        if count > 16384 {
            return Err(Error::new(
                crate::domain::diagnostics::ErrorKind::Limit,
                "HTTP route projection exceeds 16384 entries",
            ));
        }
        if inserted == 0 {
            return Ok(());
        }
    }
    Err(Error::new(
        crate::domain::diagnostics::ErrorKind::Limit,
        "HTTP mount cycle or depth limit exceeded",
    ))
}
