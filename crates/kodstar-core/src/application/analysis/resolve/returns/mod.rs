// Purpose: 생성자·명시적 반환 타입을 receiver 타입 후보로 변환.
// Pattern: Simple Module.
// Usage: 호출 결과·property receiver를 멤버 선택 전에 해석.
// Related: crates/kodstar-core/src/application/analysis/resolve/lexical.rs, crates/kodstar-core/src/application/analysis/resolve/returns/records.rs, crates/kodstar-core/src/application/analysis/resolve/python/properties.rs.

mod records;

use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use records::ReturnBinding;
use rusqlite::params;

#[derive(Clone, Copy, PartialEq)]
pub(in crate::application::analysis::resolve) enum ValueSource {
    Call,
    Property,
    Fixture,
}

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn call_result(
        &mut self,
        context: &FileContext,
        scope: u32,
        position: usize,
        producer: &str,
        depth: usize,
    ) -> Result<()> {
        self.expression(context, scope, position, producer, depth + 1)?;
        self.return_types(depth, ValueSource::Call)
    }

    pub(in crate::application::analysis::resolve) fn return_types(
        &mut self,
        depth: usize,
        source: ValueSource,
    ) -> Result<()> {
        let property_only = source == ValueSource::Property;
        let fixture = source == ValueSource::Fixture;
        if depth >= 32 {
            self.connection
                .cached_execute("DELETE FROM resolution_candidates", [])?;
            self.reason = Some("return type lookup depth limit");
            return Ok(());
        }
        self.connection.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS resolution_returns(
                depth INTEGER,id INTEGER,PRIMARY KEY(depth,id)
             );
             CREATE TEMP TABLE IF NOT EXISTS resolution_return_results(
                depth INTEGER,id INTEGER,PRIMARY KEY(depth,id)
             );",
        )?;
        self.connection
            .cached_execute("DELETE FROM resolution_returns WHERE depth=?1", [depth])?;
        self.connection.cached_execute(
            "DELETE FROM resolution_return_results WHERE depth=?1",
            [depth],
        )?;
        self.connection.cached_execute(
            "INSERT INTO resolution_returns
             SELECT ?1,c.id FROM resolution_candidates c CROSS JOIN symbols s ON s.id=c.id
             WHERE s.kind IN ('function','method') AND (
                ?2=0 OR EXISTS(SELECT 1 FROM bindings b
                    WHERE b.file_id=s.file_id AND b.target_local=s.local_id AND b.kind='python_property')
             )",
            params![depth,property_only],
        )?;
        let mut unknown: bool = self.connection.cached_row(
            "SELECT EXISTS(SELECT 1 FROM resolution_returns r JOIN symbols s ON s.id=r.id
             WHERE r.depth=?1 AND NOT EXISTS(SELECT 1 FROM bindings b
                WHERE b.file_id=s.file_id AND b.target_local=s.local_id AND (
                    b.kind='return_type' AND ?2=0
                    OR ?2 AND b.kind IN ('python_fixture_type','python_fixture_factory','python_fixture_unknown')
                )
             ))", params![depth,fixture], |r|r.get(0),
        )?;
        unknown |= self.has_externals();
        self.connection.cached_execute(
            "INSERT INTO resolution_return_results
             SELECT ?1,c.id FROM resolution_candidates c CROSS JOIN symbols s ON s.id=c.id
             WHERE s.kind IN ('class','struct','enum','type','interface','trait')
                OR (?2 AND NOT EXISTS(SELECT 1 FROM resolution_returns r WHERE r.depth=?1 AND r.id=c.id))",
            params![depth,property_only],
        )?;
        self.connection.cached_execute(
            "INSERT OR IGNORE INTO resolution_return_results
             SELECT ?1,o.id FROM resolution_candidates c CROSS JOIN symbols s ON s.id=c.id
             JOIN symbols o ON o.id=s.owner_id
             WHERE s.kind='constructor' AND o.kind IN ('class','struct')",
            [depth],
        )?;
        let mut statement = self.connection.cached_prepare(records::ANNOTATIONS)?;
        let records = statement.query_map(params![depth, fixture], ReturnBinding::read)?;
        for record in records {
            let ReturnBinding {
                file,
                scope,
                position,
                type_name,
                language,
                producer,
                kind,
            } = record?;
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
            if let Some(ty) = type_name {
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
            } else if kind == "python_fixture_unknown" {
                self.candidate_only = true;
                self.reason = Some("fixture has a return or yield without an established type");
            }
            let established: bool = self.connection.cached_row(
                "SELECT EXISTS(SELECT 1 FROM resolution_candidates c CROSS JOIN symbols s ON s.id=c.id
                 WHERE s.kind IN ('class','struct','enum','type','interface','trait'))",
                [],
                |r| r.get(0),
            )?;
            unknown |= !established;
            self.connection.cached_execute(
                "INSERT OR IGNORE INTO resolution_return_results
                 SELECT ?1,c.id FROM resolution_candidates c CROSS JOIN symbols s ON s.id=c.id
                 WHERE s.kind IN ('class','struct','enum','type','interface','trait')",
                [depth],
            )?;
        }
        self.connection
            .cached_execute("DELETE FROM resolution_candidates", [])?;
        self.connection.cached_execute("INSERT INTO resolution_candidates SELECT id FROM resolution_return_results WHERE depth=?1",[depth])?;
        self.connection
            .cached_execute("DELETE FROM resolution_returns WHERE depth=?1", [depth])?;
        self.connection.cached_execute(
            "DELETE FROM resolution_return_results WHERE depth=?1",
            [depth],
        )?;
        self.clear_externals()?;
        self.rule = "explicit_return_or_constructor_type";
        if unknown {
            self.candidate_only = true;
            self.reason = Some(
                "some producer targets have no established return type; receiver selection is incomplete",
            );
        }
        Ok(())
    }
}
