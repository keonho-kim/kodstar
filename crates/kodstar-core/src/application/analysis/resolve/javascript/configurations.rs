// Purpose: 가까운 JS/TS 설정과 상속된 alias의 디스크 보조 cache 구성.
// Pattern: Simple Module.
// Usage: 경로 해석이 현재 파일에 적용할 설정과 우선순위를 요청.
// Related: crates/kodstar-core/src/application/analysis/resolve/javascript/mod.rs, crates/kodstar-core/src/infrastructure/formats/auxiliary/configuration.rs.

use crate::application::analysis::resolve::{FileContext, Lookup, indexed_paths};
use crate::domain::diagnostics::{Error, ErrorKind, Result};
use crate::infrastructure::cache::paths::{path_bytes, path_from_bytes};
use rusqlite::{OptionalExtension, params};
use std::path::Path;

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn javascript_configuration(
        &self,
        context: &FileContext,
    ) -> Result<Option<i64>> {
        let cached: Option<Option<i64>> = self
            .connection
            .query_row(
                "SELECT configuration FROM resolution_configuration_selection WHERE file_id=?1",
                [context.id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(cached) = cached {
            return Ok(cached);
        }
        let mut selected = None;
        for directory in context
            .path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .ancestors()
        {
            for name in [
                "tsconfig.json",
                "tsconfig.jsonc",
                "jsconfig.json",
                "jsconfig.jsonc",
            ] {
                selected=self.connection.query_row("SELECT f.id FROM files f WHERE f.path=?1 AND f.status='auxiliary' AND EXISTS(SELECT 1 FROM bindings b WHERE b.file_id=f.id AND b.kind='configuration')",[path_bytes(&directory.join(name))],|r|r.get(0)).optional()?;
                if selected.is_some() {
                    break;
                }
            }
            if selected.is_some() {
                break;
            }
        }
        self.connection.execute(
            "INSERT INTO resolution_configuration_selection VALUES(?1,?2)",
            params![context.id, selected],
        )?;
        Ok(selected)
    }

    pub(in crate::application::analysis::resolve) fn load_javascript_configuration(
        &self,
        root: i64,
        id: i64,
        depth: usize,
    ) -> Result<()> {
        if self.connection.execute(
            "INSERT OR IGNORE INTO resolution_configuration_seen VALUES(?1,?2)",
            params![root, id],
        )? == 0
        {
            return Ok(());
        }
        if depth >= 32 {
            return Err(Error::new(
                ErrorKind::Limit,
                "configuration inheritance exceeds 32 levels",
            ));
        }
        // Child patterns win. Only the last declaration of a duplicate JSON key participates.
        self.connection.execute("INSERT OR IGNORE INTO resolution_path_patterns(root,pattern,configuration)
            SELECT ?1,b.name,?2 FROM bindings b WHERE b.file_id=?2 AND b.kind='path_alias'
            AND b.start_byte>(SELECT COALESCE(MAX(d.start_byte),-1) FROM bindings d WHERE d.file_id=b.file_id AND d.kind='path_alias_start' AND d.name=b.name)
            GROUP BY b.name ORDER BY MIN(b.start_byte)",params![root,id])?;
        let bytes: Vec<u8> =
            self.connection
                .query_row("SELECT path FROM files WHERE id=?1", [id], |r| r.get(0))?;
        let path = path_from_bytes(&bytes)?;
        let mut links=self.connection.prepare("SELECT name,module_name FROM bindings WHERE file_id=?1 AND kind='configuration_link' ORDER BY CASE name WHEN 'reference' THEN 0 ELSE 1 END,start_byte DESC,id DESC")?;
        let links = links.query_map([id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for link in links {
            let (kind, raw) = link?;
            if raw.starts_with('@') {
                continue;
            }
            if let Some(target) = indexed_paths::configuration_file(
                self.connection,
                path.parent().unwrap_or_else(|| Path::new("")),
                &raw,
                &kind,
            )? {
                self.load_javascript_configuration(root, target, depth + 1)?;
            }
        }
        Ok(())
    }
}
