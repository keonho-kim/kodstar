// Purpose: 증명된 이전 행의 좌표를 갱신하고 새 행·관계와 충돌 없이 보존.
// Pattern: Storage Writer.
// Usage: FactWriter가 삽입 구간 밖 사실을 처리하며 기존 행 ID만 사용.
// Related: crates/kodstar-core/src/infrastructure/index/append_reuse/mod.rs, crates/kodstar-core/src/infrastructure/index/fact_statements.rs.

use super::{FactWriter, FileFacts, Write};
use crate::{
    domain::{
        diagnostics::{Error, Result},
        facts::{
            Fact,
            append::{Membership, membership, original_bytes, span},
        },
    },
    infrastructure::index::append_reuse::RetainedRows,
};
use rusqlite::params;

impl FactWriter<'_> {
    pub(super) fn retain(
        &mut self,
        context: &mut FileFacts,
        rows: &RetainedRows,
        fact: &Fact,
    ) -> Result<bool> {
        match membership(fact, &rows.range) {
            Membership::Added => return Ok(false),
            Membership::Invalid => {
                return Err(Error::operation("facts changed after insertion proof"));
            }
            Membership::Existing => {}
        }
        let file = context.id;
        if let Fact::Module {
            name,
            declaration_only: false,
        } = fact
        {
            context.module = name.clone();
        }
        let Some(span) = span(fact) else {
            return Ok(true);
        };
        if span.end_byte <= rows.range.start {
            return Ok(true);
        }
        let (start, end) = original_bytes(span, &rows.range);
        match fact {
            Fact::Scope(scope) => self.statements.execute(
                Write::UpdateScope,
                params![span.start_byte, span.end_byte, file, scope.local_id],
            )?,
            Fact::Definition(d) => self.statements.execute(
                Write::UpdateDefinition,
                params![
                    span.start_byte,
                    span.end_byte,
                    span.line,
                    span.column,
                    span.end_line,
                    span.end_column,
                    file,
                    d.local_id
                ],
            )?,
            Fact::Binding(_) => self.statements.execute(
                Write::MoveBinding,
                params![
                    span.start_byte,
                    span.end_byte,
                    file,
                    rows.bindings,
                    start,
                    end
                ],
            )?,
            Fact::Use(_) => {
                // Only pre-existing IDs may move: a newly inserted use can occupy an old byte range.
                self.connection.execute(
                    "INSERT OR IGNORE INTO affected_facts SELECT id FROM facts_rows
                    WHERE file_id=?1 AND id<=?2 AND start_byte=?3 AND end_byte=?4",
                    params![file, rows.facts, start, end],
                )?;
                self.statements.execute(
                    Write::MoveUsage,
                    params![
                        span.start_byte,
                        span.end_byte,
                        span.line,
                        span.column,
                        span.end_line,
                        span.end_column,
                        file,
                        rows.facts,
                        start,
                        end
                    ],
                )?;
            }
            Fact::Module { .. } => {}
        }
        Ok(true)
    }
}
