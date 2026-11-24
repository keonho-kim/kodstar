// Purpose: 정의의 원본 위치·논리 식별자와 소유자 저장.
// Pattern: Storage Writer.
// Usage: FactWriter가 한 definition 사실을 위임.
// Related: crates/kodstar-core/src/infrastructure/index/facts.rs, crates/kodstar-core/src/infrastructure/index/fact_statements.rs.

use super::{FactWriter, FileFacts, Write};
use crate::domain::{diagnostics::Result, facts::Definition, languages::Language};
use rusqlite::params;

impl FactWriter<'_> {
    pub(super) fn definition(
        &mut self,
        context: &mut FileFacts,
        language: Language,
        definition: Definition,
    ) -> Result<()> {
        let file = context.id;
        if context.preserve {
            let span = definition.span;
            return self.statements.execute(
                Write::UpdateDefinition,
                params![
                    span.start_byte,
                    span.end_byte,
                    span.line,
                    span.column,
                    span.end_line,
                    span.end_column,
                    file,
                    definition.local_id
                ],
            );
        }
        let owner = definition
            .owner
            .map(|owner| self.local_symbol(file, owner))
            .transpose()?;
        let name = self.atoms.intern(&definition.name)?;
        let name_key = self.atoms.intern(&language.lookup_key(&definition.name))?;
        let qualified = self.atoms.intern(&definition.qualified_name)?;
        let kind = self.atoms.intern(definition.kind.as_str())?;
        let signature = self.atoms.intern(&definition.signature)?;
        let condition = self.atoms.optional(definition.condition.as_deref())?;
        let language_id = self.atoms.intern(language.as_str())?;
        let ordinal = self.statements.integer(
            Write::Ordinal,
            params![file, qualified, kind, signature, condition],
        )?;
        let mut hasher = blake3::Hasher::new();
        for part in [
            language.as_str().as_bytes(),
            context.package.as_bytes(),
            context.module.as_bytes(),
            context.path.as_slice(),
            definition.qualified_name.as_bytes(),
            definition.kind.as_str().as_bytes(),
            definition.signature.as_bytes(),
            definition.condition.as_deref().unwrap_or("").as_bytes(),
            &ordinal.to_le_bytes(),
        ] {
            hasher.update(&(part.len() as u64).to_le_bytes());
            hasher.update(part);
        }
        let identity = hasher.finalize();
        let span = definition.span;
        self.statements.execute(
            Write::Definition,
            params![
                file,
                definition.local_id,
                definition.scope,
                identity.as_bytes().as_slice(),
                name,
                name_key,
                qualified,
                kind,
                signature,
                condition,
                owner,
                definition.exported,
                span.start_byte,
                span.end_byte,
                span.line,
                span.column,
                span.end_line,
                span.end_column,
                language_id
            ],
        )?;

        if context.addition.is_some() {
            self.connection.execute(
                "INSERT OR IGNORE INTO append_symbols VALUES(?1)",
                [self.connection.last_insert_rowid()],
            )?;
        }
        Ok(())
    }
}
