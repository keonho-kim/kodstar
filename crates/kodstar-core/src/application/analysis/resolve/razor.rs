// Purpose: 확인 가능한 Razor namespace·using·프로젝트 문맥으로 component 파일 후보 선택.
// Pattern: Simple Module.
// Usage: 명시 component 태그의 일반 C# 이름 lookup이 실패한 뒤 호출.
// Related: crates/kodstar-core/src/infrastructure/formats/components/razor/directives.rs, crates/kodstar-core/src/application/analysis/dotnet_projects.rs.

use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::infrastructure::cache::paths::path_from_bytes;
use rusqlite::params;

impl Lookup<'_, '_> {
    pub(super) fn razor_component_file(&mut self, context: &FileContext, tag: &str) -> Result<()> {
        if self.language != Language::CSharp || context.input_format != Some(Language::Razor) {
            return Ok(());
        }
        let present: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM resolution_candidates)",
            [],
            |r| r.get(0),
        )?;
        if present {
            return Ok(());
        }
        let namespace: Option<String> = self.connection.query_row("SELECT CASE WHEN COUNT(*)=1 THEN MIN(module_name) END FROM bindings WHERE file_id=?1 AND kind='razor_namespace'", [context.id], |r|r.get(0))?;
        let (qualifier, name) = tag
            .rsplit_once('.')
            .map_or((None, tag), |(ns, name)| (Some(ns), name));
        let mut statement = self.connection.prepare(
            "SELECT s.id,f.path,(SELECT CASE WHEN COUNT(*)=1 THEN MIN(n.module_name) END
            FROM bindings n
            WHERE n.file_id=f.id
            AND n.kind='razor_namespace')
            FROM files f
            JOIN bindings b ON b.file_id=f.id
            JOIN symbols s ON s.file_id=f.id
            AND s.local_id=0
            WHERE f.package=?1
            AND b.kind='razor_component'
            AND b.imported_name=?2
            ORDER BY f.path",
        )?;
        let records = statement.query_map(params![context.package, name], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?;
        let mut found = false;
        for record in records {
            let (symbol, raw, target_namespace) = record?;
            let path = path_from_bytes(&raw)?;
            let mut proven = false;
            if let Some(target) = target_namespace.as_deref() {
                proven = qualifier == Some(target)
                    || qualifier.is_none() && namespace.as_deref() == Some(target);
                if qualifier.is_none() && !proven {
                    proven = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM bindings WHERE file_id=?1 AND scope_id=0 AND name='*' AND kind='import' AND module_name=?2 AND imported_name='*')", params![context.id,target], |r|r.get(0))?;
                }
            }
            let uncertain_same_directory = qualifier.is_none()
                && (namespace.is_none() || target_namespace.is_none())
                && path.parent() == context.path.parent();
            if !proven && !uncertain_same_directory {
                continue;
            }
            self.connection.execute(
                "INSERT OR IGNORE INTO resolution_candidates VALUES(?1)",
                [symbol],
            )?;
            found = true;
            if !proven || self.has_externals() {
                self.candidate_only = true;
                self.reason = Some(
                    "Razor default namespace or additional imported types are not established",
                );
            }
        }
        self.dependency(
            "razor_component",
            &format!("{}:{tag}", context.package),
            found,
        )?;
        if found {
            self.rule = "explicit_razor_component_file";
            if !self.candidate_only {
                self.reason = None;
            }
        }
        Ok(())
    }
}
