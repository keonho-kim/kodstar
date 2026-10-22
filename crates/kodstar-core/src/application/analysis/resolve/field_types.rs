// Purpose: 명시적 필드 타입과 factory 반환으로 receiver 후보를 제한된 깊이에서 전환.
// Pattern: Simple Module.
// Usage: 멤버 조회가 필드 후보를 타입으로 확장한 후 다음 멤버를 선택.
// Related: crates/kodstar-core/src/application/analysis/resolve/members.rs, crates/kodstar-core/src/infrastructure/parse/fields.rs.

use crate::application::analysis::resolve::Lookup;
use crate::domain::diagnostics::Result;

impl Lookup<'_, '_> {
    pub(super) fn field_types(&mut self) -> Result<()> {
        let present:bool=self.connection.cached_row("SELECT EXISTS(SELECT 1 FROM resolution_candidates c CROSS JOIN symbols s ON s.id=c.id WHERE s.kind='field')",[],|r|r.get(0))?;
        if !present {
            return Ok(());
        }
        let depth = self.member_depth;
        self.connection.execute_batch("CREATE TEMP TABLE IF NOT EXISTS resolution_fields(depth INTEGER,id INTEGER,PRIMARY KEY(depth,id)); CREATE TEMP TABLE IF NOT EXISTS resolution_field_results(depth INTEGER,id INTEGER,PRIMARY KEY(depth,id));")?;
        self.connection
            .cached_execute("DELETE FROM resolution_fields WHERE depth=?1", [depth])?;
        self.connection.cached_execute(
            "DELETE FROM resolution_field_results WHERE depth=?1",
            [depth],
        )?;
        self.connection.cached_execute("INSERT INTO resolution_fields SELECT ?1,s.id FROM resolution_candidates c CROSS JOIN symbols s ON s.id=c.id WHERE s.kind='field'",[depth])?;
        self.connection.cached_execute("INSERT INTO resolution_field_results SELECT ?1,c.id FROM resolution_candidates c CROSS JOIN symbols s ON s.id=c.id WHERE s.kind<>'field'",[depth])?;
        let mut fields = self.connection.cached_prepare(
            "SELECT s.file_id,b.scope_id,b.start_byte,CASE WHEN b.kind='swift_environment' THEN b.imported_name ELSE b.type_name END,s.language,b.imported_name,b.kind
            FROM resolution_fields c JOIN symbols s ON s.id=c.id
            JOIN bindings b ON b.file_id=s.file_id AND b.target_local=s.local_id
            WHERE c.depth=?1 AND ((b.kind='field' AND b.type_name IS NOT NULL) OR (b.kind IN ('field_factory','swift_environment') AND b.imported_name IS NOT NULL)) ORDER BY s.id",
        )?;
        let records = fields.query_map([depth], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, u32>(1)?,
                r.get::<_, usize>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, String>(6)?,
            ))
        })?;
        for record in records {
            let (file, scope, position, ty, language, producer, kind) = record?;
            let Some(context) = crate::application::analysis::resolve::context(
                self.connection,
                file,
                language.parse().ok(),
            )?
            else {
                continue;
            };
            self.connection
                .cached_execute("DELETE FROM resolution_candidates", [])?;
            if kind == "swift_environment"
                && !self.swift_environment(&context, scope, position, depth + 1)?
            {
                continue;
            }
            if let Some(ty) = ty {
                self.type_expression(
                    &context,
                    scope,
                    position,
                    crate::application::analysis::resolve::lexical::receiver_type(
                        context.language,
                        &ty,
                    ),
                    depth + 1,
                )?;
            } else if let Some(producer) = producer {
                self.call_result(&context, scope, position, &producer, depth + 1)?;
            }
            self.connection.cached_execute("INSERT OR IGNORE INTO resolution_field_results SELECT ?1,c.id FROM resolution_candidates c CROSS JOIN symbols s ON s.id=c.id WHERE s.kind IN ('class','struct','enum','type','interface','trait')",[depth])?;
        }
        self.connection
            .cached_execute("DELETE FROM resolution_candidates", [])?;
        self.connection.cached_execute("INSERT INTO resolution_candidates SELECT id FROM resolution_field_results WHERE depth=?1",[depth])?;
        self.connection
            .cached_execute("DELETE FROM resolution_fields WHERE depth=?1", [depth])?;
        self.connection.cached_execute(
            "DELETE FROM resolution_field_results WHERE depth=?1",
            [depth],
        )?;
        Ok(())
    }
}
