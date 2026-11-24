// Purpose: 미확장 native 매크로 문맥에서 해석 결과의 증명 수준과 진단을 제한.
// Pattern: Resolution Policy.
// Usage: 해석 시작과 관계 공개가 저장된 구문 경계 표식을 소비.
// Related: crates/kodstar-core/src/infrastructure/parse/cxx/macros.rs, crates/kodstar-core/src/application/analysis/resolve/publication.rs.

use super::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::infrastructure::index::cached::Sql as Connection;

pub(super) mod requirements;

pub(super) fn diagnostics(connection: &Connection, present: bool) -> Result<()> {
    connection.cached_execute(
        "DELETE FROM diagnostics WHERE code='unexpanded_native_macros'",
        [],
    )?;
    if present {
        connection.cached_execute("INSERT INTO diagnostics(file_id,code,message)
            SELECT DISTINCT file_id,'unexpanded_native_macros','Syntax-only native macro facts retain candidate bindings and ownership; proven Clang facts are reported separately'
            FROM bindings WHERE kind='native_macro_context'", [])?;
    }
    Ok(())
}

impl Lookup<'_, '_> {
    pub(super) fn native_macro_candidates(&mut self, context: &FileContext) -> Result<()> {
        if self.rule == "clang_semantic_call"
            || !self.prepared.native_macros
            || self.candidate_only
            || !matches!(context.language, Language::C | Language::Cpp)
        {
            return Ok(());
        }
        let uncertain: bool = self.connection.cached_row(
            "SELECT EXISTS(SELECT 1 FROM bindings WHERE file_id=?1 AND scope_id=0
                AND name_key='<native-macro-context>' AND kind='native_macro_context')
             OR EXISTS(SELECT 1 FROM resolution_candidates c CROSS JOIN symbols s ON s.id=c.id
                CROSS JOIN bindings b ON b.file_id=s.file_id AND b.scope_id=0
                AND b.name_key='<native-macro-context>' AND b.kind='native_macro_context')",
            [context.id],
            |row| row.get(0),
        )?;
        if uncertain {
            self.candidate_only = true;
            self.reason = Some("unexpanded native macros may change binding or ownership");
        }
        Ok(())
    }
}
