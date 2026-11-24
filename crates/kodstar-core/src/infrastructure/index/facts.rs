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

mod definitions;
mod retained;

pub(crate) struct FileFacts {
    pub(crate) id: i64,
    path: Vec<u8>,
    package: String,
    pub(super) module: String,
    preserve: bool,
    pub(crate) call_reuse: bool,
    pub(crate) addition: Option<super::append_reuse::RetainedRows>,
}
impl FileFacts {
    pub(crate) fn preserves_definitions(&self) -> bool {
        self.preserve
    }
    pub(crate) fn new(id: i64, record: &FileRecord, preserve: bool) -> Self {
        Self {
            id,
            path: path_bytes(&record.path),
            package: record.package.clone(),
            module: record.module.clone(),
            preserve,
            call_reuse: false,
            addition: None,
        }
    }
}

pub(crate) struct FactWriter<'conn> {
    connection: &'conn Connection,
    body: Option<super::body_reuse::Matcher<'conn>>,
    statements: Statements<'conn>,
    atoms: super::atoms::writer::Writer<'conn>,
}
impl<'conn> FactWriter<'conn> {
    pub(crate) fn new(connection: &'conn Connection) -> Result<Self> {
        Ok(Self {
            connection,
            body: None,
            statements: Statements::new(connection)?,
            atoms: super::atoms::writer::Writer::new(connection)?,
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
        if let Some(rows) = context.addition.clone()
            && self.retain(context, &rows, &fact)?
        {
            return Ok(());
        }
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
            Fact::Definition(definition) => self.definition(context, language, definition)?,
            Fact::Binding(binding) => {
                if context.preserve && binding.kind != "clang_call" {
                    if matches!(binding.kind, "function" | "go_signature") {
                        self.statements.execute(
                            Write::UpdateCallableSpan,
                            params![
                                binding.span.start_byte,
                                binding.span.end_byte,
                                file,
                                binding.target,
                                binding.kind
                            ],
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
                let key = self.atoms.intern(&language.lookup_key(&binding.name))?;
                let name = self.atoms.intern(&binding.name)?;
                let module = self.atoms.optional(binding.module.as_deref())?;
                let imported = self.atoms.optional(binding.imported_name.as_deref())?;
                let type_name = self.atoms.optional(binding.type_name.as_deref())?;
                let kind = self.atoms.intern(binding.kind)?;
                let namespace = self.atoms.intern(namespace)?;
                self.statements.execute(
                    Write::Binding,
                    params![
                        file,
                        binding.scope,
                        name,
                        key,
                        binding.target,
                        module,
                        imported,
                        type_name,
                        kind,
                        binding.span.start_byte,
                        binding.span.end_byte,
                        namespace
                    ],
                )?;
            }
            Fact::Use(usage) => {
                if context.preserve
                    && !matches!(
                        usage.relation,
                        crate::domain::relations::RelationKind::Calls
                            | crate::domain::relations::RelationKind::References
                    )
                {
                    return Ok(());
                }
                let source = self.local_symbol(file, usage.source)?;
                let span = usage.span;
                let expression = self.atoms.intern(&usage.expression)?;
                let relation = self.atoms.intern(usage.relation.as_str())?;
                let condition = self.atoms.optional(usage.condition.as_deref())?;
                let language_id = self.atoms.intern(language.as_str())?;
                if context.call_reuse {
                    let values = [
                        file,
                        source,
                        i64::from(usage.scope),
                        expression,
                        relation,
                        condition.unwrap_or(0),
                        i64::from(usage.type_only),
                        span.start_byte as i64,
                        span.end_byte as i64,
                        span.line as i64,
                        span.column as i64,
                        span.end_line as i64,
                        span.end_column as i64,
                        language_id,
                        if matches!(language, Language::C | Language::Cpp)
                            && usage.relation == crate::domain::relations::RelationKind::Calls
                        {
                            self.statements.integer(
                                Write::NativeCallModule,
                                params![file, usage.scope, span.start_byte, span.end_byte],
                            )?
                        } else {
                            0
                        },
                    ];
                    let matcher = match &mut self.body {
                        Some(matcher) => matcher,
                        slot @ None => slot.insert(super::body_reuse::Matcher::new(
                            self.connection,
                            (self
                                .connection
                                .limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH)?
                                as usize)
                                .saturating_sub(self.statements.memory_bytes()),
                        )?),
                    };
                    if matcher.matches(&values)? {
                        return Ok(());
                    }
                }
                self.statements.execute(
                    Write::Usage,
                    params![
                        file,
                        source,
                        usage.scope,
                        expression,
                        relation,
                        condition,
                        usage.type_only,
                        span.start_byte,
                        span.end_byte,
                        span.line,
                        span.column,
                        span.end_line,
                        span.end_column,
                        language_id
                    ],
                )?;
                if context.call_reuse || context.addition.is_some() {
                    self.connection.execute(
                        "INSERT OR IGNORE INTO affected_facts VALUES(?1)",
                        [self.connection.last_insert_rowid()],
                    )?;
                }
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
                    params![self.atoms.intern(&name)?, file],
                )?;
                if !declaration_only {
                    context.module = name;
                }
            }
        }
        Ok(())
    }
}
