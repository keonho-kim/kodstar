// Purpose: 인덱스 내부 경로와 입력 종류별 파일 후보 선택.
// Pattern: Simple Module.
// Usage: module·설정 경로 해석과 명시적 설정 확장에서 호출.
// Related: crates/kodstar-core/src/application/analysis/resolve/modules/mod.rs, crates/kodstar-core/src/application/analysis/configurations.rs.

use crate::application::analysis::resolve::Lookup;
use crate::domain::diagnostics::Result;
use crate::infrastructure::cache::paths::path_bytes;
use crate::infrastructure::index::cached::Sql as Connection;
use rusqlite::{OptionalExtension, params};
use std::path::{Component, Path, PathBuf};

pub(crate) enum PathKind {
    JavaScript,
    Include,
    Ruby,
    Zig,
    ErlangInclude,
    Pascal,
}

pub(crate) fn normalize(parent: &Path, raw: &str) -> Option<PathBuf> {
    if raw.contains(['\0', ':']) {
        return None;
    }
    let mut path = PathBuf::new();
    for component in parent.join(raw).components() {
        match component {
            Component::Normal(name) => path.push(name),
            Component::CurDir => {}
            Component::ParentDir if path.pop() => {}
            _ => return None,
        }
    }
    Some(path)
}

pub(crate) fn configuration_file(
    connection: &Connection,
    parent: &Path,
    raw: &str,
    kind: &str,
) -> Result<Option<i64>> {
    let Some(path) = normalize(parent, raw) else {
        return Ok(None);
    };
    let extension = if path.extension().is_none() {
        path.with_extension("json")
    } else {
        path.clone()
    };
    let candidates = if kind == "reference" {
        [path.clone(), path.join("tsconfig.json"), extension]
    } else {
        [path.clone(), extension, path.join("tsconfig.json")]
    };
    for candidate in candidates {
        if let Some(id) = connection
            .cached_row(
                "SELECT id FROM files WHERE path=?1",
                [path_bytes(&candidate)],
                |r| r.get(0),
            )
            .optional()?
        {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

impl Lookup<'_, '_> {
    pub(super) fn indexed_path(&self, path: &Path, kind: PathKind, depth: usize) -> Result<bool> {
        let provider = match kind {
            PathKind::JavaScript => Some(crate::domain::resolve::lookup_keys::path_provider(
                "javascript",
                &path_bytes(&crate::domain::resolve::lookup_keys::javascript_path(path)),
            )),
            PathKind::Zig => Some(crate::domain::resolve::lookup_keys::path_provider(
                "zig",
                &path_bytes(path),
            )),
            _ => None,
        };
        let (extensions, allowed): (&[&str], &str) = match kind {
            PathKind::JavaScript => (
                &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"],
                "'javascript','typescript','vue','svelte','astro'",
            ),
            PathKind::Include => (&[], "'c','cpp'"),
            PathKind::Ruby => (&["rb"], "'ruby'"),
            PathKind::Zig => (&[], "'zig'"),
            PathKind::ErlangInclude => (&[], "'erlang'"),
            PathKind::Pascal => (&["pas", "pp"], "'pascal'"),
        };
        let extensions = if path.extension().is_none()
            || matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("js" | "jsx" | "mjs" | "cjs")
            ) {
            extensions
        } else {
            &[]
        };
        let candidates = std::iter::once(path.to_path_buf())
            .chain(
                extensions
                    .iter()
                    .map(|extension| path.with_extension(extension)),
            )
            .chain(
                extensions
                    .iter()
                    .filter(|_| matches!(kind, PathKind::JavaScript))
                    .map(|extension| path.join(format!("index.{extension}"))),
            );
        let sql = format!(
            "SELECT id FROM files WHERE path=?1 AND status='analyzed' AND language IN ({allowed})"
        );
        let mut statement = self.connection.cached_prepare(&sql)?;
        for candidate in candidates {
            if let Some(id) = statement
                .query_row([path_bytes(&candidate)], |r| r.get::<_, i64>(0))
                .optional()?
            {
                self.connection.cached_execute(
                    "INSERT OR IGNORE INTO resolution_modules VALUES(?1,?2)",
                    params![depth, id],
                )?;
                if let Some(key) = &provider {
                    self.dependency("path_provider", key, true)?;
                }
                return Ok(true);
            }
        }
        if let Some(key) = &provider {
            self.dependency("path_provider", key, false)?;
        }
        Ok(false)
    }
}
