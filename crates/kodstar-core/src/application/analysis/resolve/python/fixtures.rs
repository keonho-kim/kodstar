// Purpose: 테스트·fixture의 untyped 매개변수를 가까운 pytest fixture와 연결.
// Pattern: Simple Module.
// Usage: Python lexical receiver가 실제 parameter binding을 확인한 뒤 호출.
// Related: crates/kodstar-core/src/application/analysis/resolve/lexical.rs, crates/kodstar-core/src/application/analysis/resolve/returns/mod.rs.

use crate::application::analysis::resolve::returns::ValueSource;
use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::infrastructure::cache::paths::path_bytes;
use rusqlite::{OptionalExtension, params};
use std::path::Path;

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn pytest_fixture(
        &mut self,
        context: &FileContext,
        scope: u32,
        name: &str,
        depth: usize,
    ) -> Result<bool> {
        if context.language != Language::Python || depth >= 32 {
            return Ok(false);
        }
        let filename = context
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let test_module = filename == "conftest.py"
            || filename.starts_with("test_") && filename.ends_with(".py")
            || filename.ends_with("_test.py");
        if !test_module {
            return Ok(false);
        }
        let eligible:bool=self.connection.query_row("SELECT EXISTS(SELECT 1 FROM symbols s WHERE s.file_id=?1 AND s.local_id=?2 AND (s.name LIKE 'test%' OR EXISTS(SELECT 1 FROM bindings b WHERE b.file_id=s.file_id AND b.target_local=s.local_id AND b.kind='python_fixture')))",params![context.id,scope],|r|r.get(0))?;
        if !eligible {
            return Ok(false);
        }
        if self.fixture_in_file(context.id, name, depth)? {
            return Ok(true);
        }
        let mut directory = context
            .path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf();
        loop {
            let path = directory.join("conftest.py");
            if path != context.path {
                let file: Option<i64> = self
                    .connection
                    .query_row(
                        "SELECT id FROM files WHERE path=?1",
                        [path_bytes(&path)],
                        |r| r.get(0),
                    )
                    .optional()?;
                if let Some(file) = file
                    && self.fixture_in_file(file, name, depth)?
                {
                    return Ok(true);
                }
            }
            if !directory.pop() {
                break;
            }
        }
        Ok(false)
    }

    fn fixture_in_file(&mut self, file: i64, name: &str, depth: usize) -> Result<bool> {
        let mut statement = self.connection.prepare(
            "SELECT s.id,s.scope_id,b.imported_name,b.start_byte
            FROM bindings b
            JOIN symbols s ON s.file_id=b.file_id
            AND s.local_id=b.target_local
            JOIN symbols owner ON owner.id=s.owner_id
            WHERE b.file_id=?1
            AND b.name=?2
            AND b.kind='python_fixture'
            AND owner.kind='file'
            ORDER BY b.start_byte DESC LIMIT 1",
        )?;
        let fixture = statement
            .query_row(params![file, name], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, u32>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, usize>(3)?,
                ))
            })
            .optional()?;
        let Some((id, scope, decorator, position)) = fixture else {
            return Ok(false);
        };
        let Some(context) = crate::application::analysis::resolve::context(
            self.connection,
            file,
            Some(Language::Python),
        )?
        else {
            return Ok(false);
        };
        let (head, member) = decorator
            .split_once('.')
            .map_or((decorator.as_str(), None), |(h, m)| (h, Some(m)));
        let Some(binding) = self.binding(&context, scope, position, head)? else {
            return Ok(false);
        };
        let valid = matches!(binding.module.as_deref(), Some("pytest" | "pytest_asyncio"))
            && if member == Some("fixture") {
                binding.imported.is_none()
            } else {
                member.is_none() && binding.imported.as_deref() == Some("fixture")
            };
        if !valid {
            return Ok(false);
        }
        self.connection.execute(
            "INSERT OR IGNORE INTO resolution_candidates VALUES(?1)",
            [id],
        )?;
        self.return_types(depth, ValueSource::Fixture)?;
        self.rule = "pytest_fixture_return_type";
        self.reason = Some("nearest fixture return type is not statically established");
        Ok(true)
    }
}
