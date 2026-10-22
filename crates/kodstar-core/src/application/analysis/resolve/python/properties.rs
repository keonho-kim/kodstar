// Purpose: Python property 읽기 대상을 getter 정의로 제한.
// Pattern: Simple Module.
// Usage: property marker·후속 멤버 lookup이 호출.
// Related: crates/kodstar-core/src/application/analysis/resolve/inputs.rs, crates/kodstar-core/src/application/analysis/resolve/returns/mod.rs.

use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use rusqlite::params;

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn python_property(
        &mut self,
        context: &FileContext,
        expression: &str,
    ) -> Result<()> {
        let (scope, position): (u32, usize) = self.connection.cached_row(
            "SELECT scope_id,start_byte FROM facts WHERE id=?1",
            [self.fact],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        self.expression(context, scope, position, expression, 0)?;
        self.connection.cached_execute("DELETE FROM resolution_candidates WHERE NOT EXISTS(SELECT 1 FROM symbols s JOIN bindings b ON b.file_id=s.file_id AND b.target_local=s.local_id WHERE s.id=resolution_candidates.id AND b.kind='python_property')",[])?;
        self.verified_properties()?;
        self.clear_externals()?;
        self.rule = "python_property_read";
        Ok(())
    }

    pub(in crate::application::analysis::resolve) fn property_types(&mut self) -> Result<()> {
        if self.language != Language::Python {
            return Ok(());
        }
        self.verified_properties()?;
        let present:bool=self.connection.cached_row("SELECT EXISTS(SELECT 1 FROM resolution_candidates c CROSS JOIN symbols s ON s.id=c.id JOIN bindings b ON b.file_id=s.file_id AND b.target_local=s.local_id WHERE b.kind='python_property')",[],|r|r.get(0))?;
        if present {
            self.return_types(
                self.member_depth,
                crate::application::analysis::resolve::returns::ValueSource::Property,
            )?;
        }
        Ok(())
    }

    fn verified_properties(&mut self) -> Result<()> {
        self.connection.execute_batch("CREATE TEMP TABLE IF NOT EXISTS resolution_properties(depth INTEGER,id INTEGER,PRIMARY KEY(depth,id));")?;
        let depth = self.member_depth;
        self.connection
            .cached_execute("DELETE FROM resolution_properties WHERE depth=?1", [depth])?;
        self.connection.cached_execute("INSERT INTO resolution_properties SELECT DISTINCT ?1,c.id FROM resolution_candidates c CROSS JOIN symbols s ON s.id=c.id JOIN bindings b ON b.file_id=s.file_id AND b.target_local=s.local_id WHERE b.kind='python_property'",[depth])?;
        let mut statement = self.connection.cached_prepare(
            "SELECT s.id,s.file_id,s.scope_id,b.start_byte,b.imported_name
            FROM resolution_properties p
            JOIN symbols s ON s.id=p.id
            JOIN bindings b ON b.file_id=s.file_id
            AND b.target_local=s.local_id
            WHERE p.depth=?1
            AND b.kind='python_property'
            ORDER BY s.id,b.id",
        )?;
        let records = statement.query_map([depth], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, u32>(2)?,
                r.get::<_, usize>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        for record in records {
            let (id, file, scope, position, decorator) = record?;
            let Some(context) = crate::application::analysis::resolve::context(
                self.connection,
                file,
                Some(Language::Python),
            )?
            else {
                continue;
            };
            let (head, member) = decorator
                .split_once('.')
                .map_or((decorator.as_str(), None), |(h, m)| (h, Some(m)));
            let binding = self.binding(&context, scope, position, head)?;
            let valid = match (head, member, binding) {
                ("property", None, None) => true,
                (_, None, Some(b)) => {
                    b.module.as_deref() == Some("functools")
                        && b.imported.as_deref() == Some("cached_property")
                }
                (_, Some("cached_property"), Some(b)) => {
                    b.module.as_deref() == Some("functools") && b.imported.is_none()
                }
                _ => false,
            };
            if !valid {
                self.connection
                    .cached_execute("DELETE FROM resolution_candidates WHERE id=?1", [id])?;
            }
        }
        self.connection.cached_execute(
            "DELETE FROM resolution_properties WHERE depth=?1",
            params![depth],
        )?;
        Ok(())
    }
}
