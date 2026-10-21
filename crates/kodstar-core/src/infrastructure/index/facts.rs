// Purpose: 파일 문맥을 유지하며 준비된 SQL로 구문 fact를 저장.
// Pattern: Storage Adapter.
// Usage: pipeline과 configuration 추출이 transaction 안에서 writer를 소유.
// Related: crates/kodstar-core/src/infrastructure/index/fact_statements.rs, crates/kodstar-core/src/infrastructure/index/write.rs.

use super::{
    FileRecord,
    fact_statements::{Statements, Write},
};
use crate::domain::diagnostics::{Error, Result};
use crate::domain::facts::Fact;
use crate::domain::languages::Language;
use crate::infrastructure::cache::paths::path_bytes;
use rusqlite::{Connection, params};

pub(crate) struct FileFacts {
    pub(crate) id: i64,
    path: Vec<u8>,
    package: String,
    module: String,
    preserve: bool,
}
impl FileFacts {
    pub(crate) fn new(id: i64, record: &FileRecord, preserve: bool) -> Self {
        Self {
            id,
            path: path_bytes(&record.path),
            package: record.package.clone(),
            module: record.module.clone(),
            preserve,
        }
    }
}

pub(crate) struct FactWriter<'conn> {
    statements: Statements<'conn>,
}
impl<'conn> FactWriter<'conn> {
    pub(crate) fn new(connection: &'conn Connection) -> Result<Self> {
        Ok(Self {
            statements: Statements::new(connection)?,
        })
    }
    fn local_symbol(&mut self, file: i64, local: u32) -> Result<i64> {
        self.statements
            .integer(Write::LocalSymbol, params![file, local])
            .map_err(|error| {
                let kind = error.kind;
                let mut wrapped = Error::caused_by("read file-local symbol for syntax fact", error);
                wrapped.kind = kind;
                wrapped
            })
    }
    pub(crate) fn persist(
        &mut self,
        context: &mut FileFacts,
        language: Language,
        fact: Fact,
    ) -> Result<()> {
        let file = context.id;
        match fact {
            Fact::Scope(scope) => {
                if context.preserve {
                    return self.statements.execute(
                        Write::UpdateScope,
                        params![
                            scope.span.start_byte,
                            scope.span.end_byte,
                            file,
                            scope.local_id
                        ],
                    );
                }
                self.statements.execute(
                    Write::Scope,
                    params![
                        file,
                        scope.local_id,
                        scope.parent,
                        scope.span.start_byte,
                        scope.span.end_byte
                    ],
                )?;
            }
            Fact::Definition(definition) => {
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
                let ordinal = self.statements.integer(
                    Write::Ordinal,
                    params![
                        file,
                        definition.qualified_name,
                        definition.kind.as_str(),
                        definition.signature,
                        definition.condition
                    ],
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
                let name_key = language.lookup_key(&definition.name);
                let span = definition.span;
                self.statements.execute(
                    Write::Definition,
                    params![
                        file,
                        definition.local_id,
                        definition.scope,
                        hasher.finalize().to_hex().to_string(),
                        definition.name,
                        name_key,
                        definition.qualified_name,
                        definition.kind.as_str(),
                        definition.signature,
                        definition.condition,
                        owner,
                        definition.exported,
                        span.start_byte,
                        span.end_byte,
                        span.line,
                        span.column,
                        span.end_line,
                        span.end_column,
                        language.as_str()
                    ],
                )?;
            }
            Fact::Binding(binding) => {
                if context.preserve {
                    if binding.kind == "function" {
                        self.statements.execute(
                            Write::UpdateFunctionEnd,
                            params![binding.span.end_byte, file, binding.target],
                        )?;
                    }
                    return Ok(());
                }
                let namespace = crate::domain::resolve::namespaces::Namespace::binding(
                    language,
                    binding.kind,
                    &binding.name,
                )
                .as_str();
                let key = language.lookup_key(&binding.name);
                self.statements.execute(
                    Write::Binding,
                    params![
                        file,
                        binding.scope,
                        binding.name,
                        key,
                        binding.target,
                        binding.module,
                        binding.imported_name,
                        binding.type_name,
                        binding.kind,
                        binding.span.start_byte,
                        binding.span.end_byte,
                        namespace
                    ],
                )?;
            }
            Fact::Use(usage) => {
                let source = self.local_symbol(file, usage.source)?;
                let span = usage.span;
                self.statements.execute(
                    Write::Usage,
                    params![
                        file,
                        source,
                        usage.scope,
                        usage.expression,
                        usage.relation.as_str(),
                        usage.condition,
                        usage.type_only,
                        span.start_byte,
                        span.end_byte,
                        span.line,
                        span.column,
                        span.end_line,
                        span.end_column,
                        language.as_str()
                    ],
                )?;
            }
            Fact::Module {
                name,
                declaration_only,
            } => {
                if context.preserve {
                    return Ok(());
                }
                self.statements.execute(
                    if declaration_only {
                        Write::DeclaredModule
                    } else {
                        Write::Module
                    },
                    params![name, file],
                )?;
                if !declaration_only {
                    context.module = name;
                }
            }
        }
        Ok(())
    }
}
