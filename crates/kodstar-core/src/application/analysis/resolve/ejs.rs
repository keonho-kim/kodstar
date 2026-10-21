// Purpose: EJS server include와 격리된 client scope·렌더링 후보 정책.
// Pattern: Simple Module.
// Usage: fact lookup이 입력 marker와 실행 문맥을 판별할 때 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/javascript/ejs.rs, crates/kodstar-core/src/application/analysis/resolve/inputs.rs.

use crate::application::analysis::resolve::inputs::Disposition;
use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::infrastructure::cache::paths::path_bytes;
use rusqlite::{Connection, OptionalExtension, params};

pub(super) fn diagnostics(connection: &Connection) -> Result<()> {
    connection.execute(
        "DELETE FROM diagnostics WHERE code='ejs_rendered_client'",
        [],
    )?;
    connection.execute("INSERT INTO diagnostics(file_id,code,message)
        SELECT f.id,'ejs_rendered_client','client source contains EJS runtime rendering; source syntax was analyzed and dependent relations remain candidates'
        FROM files f
        WHERE f.language='ejs'
        AND EXISTS(SELECT 1
        FROM bindings b
        WHERE b.file_id=f.id
        AND b.kind='ejs_client'
        AND b.type_name IS NOT NULL)",[])?;
    Ok(())
}

impl Lookup<'_, '_> {
    fn ejs_client(&self, context: &FileContext, scope: u32) -> Result<Option<bool>> {
        Ok(self.connection.query_row(
            "WITH RECURSIVE parents(id,depth) AS (SELECT ?2,0
            UNION ALL
            SELECT s.parent,p.depth+1
            FROM scopes s
            JOIN parents p ON s.local_id=p.id
            WHERE s.file_id=?1
            AND s.parent IS NOT NULL
            AND p.depth<1024)
            SELECT MAX(b.type_name IS NOT NULL)
            FROM parents p
            JOIN bindings b ON b.file_id=?1
            AND b.scope_id=p.id
            WHERE b.kind='ejs_client'",
            params![context.id, scope],
            |r| r.get(0),
        )?)
    }
    pub(super) fn ejs_context(&mut self, context: &FileContext, scope: u32) -> Result<()> {
        if context.input_format == Some(Language::Ejs)
            && !matches!(self.relation, "contains" | "defines")
            && self.ejs_client(context, scope)? == Some(true)
        {
            self.candidate_only = true;
            self.reason = Some(
                "client code contains EJS runtime rendering; generated JavaScript is not established",
            );
        }
        Ok(())
    }
    pub(super) fn ejs_input(
        &mut self,
        context: &FileContext,
        expression: &str,
    ) -> Result<Option<Disposition>> {
        if context.input_format != Some(Language::Ejs) {
            return Ok(None);
        }
        let include = expression.starts_with("@ejs-include:");
        if !include && !(self.relation == "calls" && expression == "include") {
            return Ok(None);
        }
        let (scope, position): (u32, usize) = self.connection.query_row(
            "SELECT scope_id,start_byte FROM facts WHERE id=?1",
            [self.fact],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if self.ejs_client(context, scope)?.is_some() {
            return Ok(if include {
                Some(Disposition::Discarded)
            } else {
                None
            });
        }
        if self.binding(context, scope, position, "include")?.is_some() {
            return Ok(if include {
                Some(Disposition::Discarded)
            } else {
                None
            });
        }
        if !include {
            self.external_target("builtin:ejs:include")?;
            self.rule = "ejs_builtin_include";
            return Ok(Some(Disposition::Handled));
        }
        self.rule = "explicit_ejs_include";
        let Some(raw) = expression
            .strip_prefix("@ejs-include:literal:")
            .filter(|s| !s.is_empty())
        else {
            self.reason = Some("EJS include path is dynamic or empty");
            return Ok(Some(Disposition::Handled));
        };
        let parent = context
            .path
            .parent()
            .unwrap_or_else(|| std::path::Path::new(""));
        let path = crate::application::analysis::resolve::indexed_paths::normalize(parent, raw);
        let Some(path) = path else {
            self.reason = Some(
                "EJS include requires a contained relative path; root/views options are not evaluated",
            );
            return Ok(Some(Disposition::Handled));
        };
        let path = if path.extension().is_none() {
            path.with_extension("ejs")
        } else {
            path
        };
        let target:Option<i64>=self.connection.query_row("SELECT s.id FROM files f JOIN symbols s ON s.file_id=f.id AND s.local_id=0 WHERE f.path=?1 AND f.language='ejs' AND f.status='analyzed'",[path_bytes(&path)],|r|r.get(0)).optional()?;
        self.dependency("ejs_include", raw, target.is_some())?;
        if let Some(target) = target {
            self.connection.execute(
                "INSERT OR IGNORE INTO resolution_candidates VALUES(?1)",
                [target],
            )?;
        } else {
            self.reason = Some("EJS include file is missing, excluded or not indexed");
        }
        Ok(Some(Disposition::Handled))
    }
}
