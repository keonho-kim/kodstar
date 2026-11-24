// Purpose: 해석 작업의 고정 SQL과 불변 표식 판정의 수명·메모리 관리.
// Pattern: Simple Module.
// Usage: resolve가 생성하고 binding·후보·증거·관계 저장이 빌림.
// Related: crates/kodstar-core/src/application/analysis/resolve/mod.rs, crates/kodstar-core/src/application/analysis/resolve/publication.rs, crates/kodstar-core/src/application/analysis/resolve/prepared/dependencies.rs.

use crate::application::analysis::resolve::Binding;
use crate::domain::diagnostics::{Error, ErrorKind, Result};
use rusqlite::{Connection, OptionalExtension, Params, Statement, StatementStatus};
use std::cell::{Cell, RefCell};

mod dependencies;

#[derive(Clone, Copy)]
pub(super) enum Publication {
    Internal,
    External,
    Unresolved,
}

pub(super) struct Prepared<'conn> {
    pub(super) native_macros: bool,
    pub(super) native_calls: bool,
    pub(super) partial_sources: Cell<bool>,
    pub(super) module_exports: bool,
    pub(super) ruby_barriers: Cell<Option<super::ruby::Barriers>>,
    pub(super) reuse: RefCell<super::reuse::Cache>,
    binding: RefCell<Statement<'conn>>,
    binding_exact: RefCell<Statement<'conn>>,
    candidates: RefCell<Statement<'conn>>,
    rust: RefCell<Statement<'conn>>,
    dependency: RefCell<Statement<'conn>>,
    expressions: RefCell<dependencies::Journal<'conn>>,
    empty: Cell<Option<(bool, bool)>>,
    clear: [RefCell<Statement<'conn>>; 2],
    counts: RefCell<Statement<'conn>>,
    publications: [RefCell<Statement<'conn>>; 6],
    bytes: Cell<usize>,
    limit: usize,
}

impl<'conn> Prepared<'conn> {
    pub(super) fn new(c: &'conn Connection) -> Result<Self> {
        let binding = RefCell::new(c.prepare(include_str!("prepared/bindings.sql"))?);
        let binding_exact =
            RefCell::new(c.prepare(&include_str!("prepared/bindings.sql").replace(
                "/* exact binding guard */",
                "AND name_key_id=(SELECT id FROM atoms WHERE value=?3)",
            ))?);
        let candidates = RefCell::new(c.prepare(include_str!("prepared/candidates.sql"))?);
        let rust = RefCell::new(c.prepare(include_str!("prepared/rust.sql"))?);
        let dependency=RefCell::new(c.prepare("INSERT OR REPLACE INTO resolution_dependencies(fact_id,kind,lookup_key,found) VALUES(?1,?2,?3,?4)")?);
        let clear = [
            RefCell::new(c.prepare("DELETE FROM resolution_candidates")?),
            RefCell::new(c.prepare("DELETE FROM resolution_externals")?),
        ];
        let counts = RefCell::new(c.prepare("SELECT (SELECT COUNT(*) FROM (SELECT 1 FROM resolution_candidates LIMIT 2)),(SELECT COUNT(*) FROM (SELECT 1 FROM resolution_externals WHERE scope=0 LIMIT 2))")?);
        let mut publications = Vec::with_capacity(6);
        for table in ["resolution_relationships", "relationships"] {
            for body in [
                "(fact_id,source_id,target_id,resolution,rule,reason) SELECT ?1,?2,id,?3,?4,?5 FROM resolution_candidates",
                "(fact_id,source_id,external_key,resolution,rule,reason) SELECT ?1,?2,key,?3,?4,?5 FROM resolution_externals WHERE scope=0",
                "(fact_id,source_id,resolution,rule,reason) VALUES(?1,?2,'unresolved',?3,?4)",
            ] {
                publications.push(RefCell::new(
                    c.prepare(&format!("INSERT INTO {table}{body}"))?,
                ));
            }
        }
        let bytes = [&binding, &binding_exact, &candidates, &rust, &dependency]
            .into_iter()
            .chain(clear.iter())
            .chain([&counts])
            .chain(publications.iter())
            .map(|s| s.borrow().get_status(StatementStatus::MemUsed) as usize)
            .fold(0usize, usize::saturating_add);
        let limit = c.limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH)? as usize;
        if bytes > limit {
            return Err(limit_error());
        }
        let reuse_limit = (limit / 64).min(65536).min((limit - bytes) / 4);
        let expressions = dependencies::Journal::new(c, limit);
        let expression_bytes = expressions.limit;
        Ok(Self {
            partial_sources: Cell::new(false),
            native_calls: c.query_row("SELECT EXISTS(SELECT 1 FROM atoms WHERE value='clang_call')", [], |r|r.get(0))?,
            native_macros: c.query_row(
                "SELECT EXISTS(SELECT 1 FROM bindings WHERE kind='native_macro_context')",
                [],
                |row| row.get(0),
            )?,
            module_exports: c.query_row("SELECT EXISTS(SELECT 1 FROM bindings_rows WHERE kind IN
                (SELECT id FROM atoms WHERE value IN ('export','export_type','commonjs_export_module','commonjs_export_exports')))", [], |row|row.get(0))?,
            ruby_barriers: Cell::new(None),
            reuse: RefCell::new(super::reuse::Cache::new(reuse_limit)),
            binding,
            binding_exact,
            candidates,
            rust,
            dependency,
            expressions: RefCell::new(expressions),
            empty: Cell::new(None),
            clear,
            counts,
            publications: publications
                .try_into()
                .map_err(|_| Error::operation("invalid fixed publication query count"))?,
            bytes: Cell::new(bytes + reuse_limit + expression_bytes),
            limit,
        })
    }

    pub(super) fn binding<P: Params>(&self, params: P, exact: bool) -> Result<Option<Binding>> {
        let query = if exact {
            &self.binding_exact
        } else {
            &self.binding
        };
        let mut statement = query
            .try_borrow_mut()
            .map_err(|_| Error::operation("binding query is already active"))?;
        let before = statement.get_status(StatementStatus::MemUsed) as usize;
        let result = statement
            .query_row(params, |r| {
                Ok(Binding {
                    scope: r.get(0)?,
                    target: r.get(1)?,
                    module: r.get(2)?,
                    imported: r.get(3)?,
                    type_name: r.get(4)?,
                    kind: r.get(5)?,
                    position: r.get(6)?,
                })
            })
            .optional();
        statement.clear_bindings();
        self.check(
            before,
            statement.get_status(StatementStatus::MemUsed) as usize,
        )?;
        Ok(result?)
    }

    pub(super) fn candidates<P: Params>(&self, params: P) -> Result<usize> {
        self.start_lookup();
        self.execute(&self.candidates, params)
    }

    pub(super) fn start_lookup(&self) {
        self.empty.set(None);
    }
    pub(super) fn clear(&self) -> Result<()> {
        let empty = self.empty.replace(None);
        for (index, statement) in self.clear.iter().enumerate() {
            if empty
                .is_some_and(|(internal, external)| if index == 0 { internal } else { external })
            {
                continue;
            }
            self.execute(statement, [])?;
        }
        Ok(())
    }
    pub(super) fn counts(&self) -> Result<(i64, i64)> {
        let mut statement = self
            .counts
            .try_borrow_mut()
            .map_err(|_| Error::operation("candidate counts are already active"))?;
        let before = statement.get_status(StatementStatus::MemUsed) as usize;
        let result = statement.query_row([], |r| Ok((r.get(0)?, r.get(1)?)));
        self.check(
            before,
            statement.get_status(StatementStatus::MemUsed) as usize,
        )?;
        let counts: (i64, i64) = result?;
        // External nested scopes must also be empty before skipping their reset.
        self.empty.set(Some((counts.0 == 0, false)));
        Ok(counts)
    }

    pub(super) fn expression(&self, fact: i64, key: String, found: bool) -> Result<()> {
        self.expressions.borrow_mut().record(fact, key, found)
    }

    pub(super) fn flush_dependencies(&self) -> Result<()> {
        self.expressions.borrow_mut().flush()
    }

    pub(super) fn report(&self) {
        tracing::info!(
            expression_rows = self.expressions.borrow().rows,
            expression_statements = self.expressions.borrow().statements,
            expression_steps = self.expressions.borrow().steps,
            binding_steps = self.binding.borrow().get_status(StatementStatus::VmStep),
            exact_binding_steps = self
                .binding_exact
                .borrow()
                .get_status(StatementStatus::VmStep),
            candidate_steps = self.candidates.borrow().get_status(StatementStatus::VmStep),
            dependency_steps = self.dependency.borrow().get_status(StatementStatus::VmStep),
            "resolution SQL work"
        );
    }
    pub(super) fn rust<P: Params>(&self, params: P) -> Result<usize> {
        self.start_lookup();
        self.execute(&self.rust, params)
    }
    pub(super) fn dependency<P: Params>(&self, params: P) -> Result<usize> {
        self.execute(&self.dependency, params)
    }
    pub(super) fn publish<P: Params>(
        &self,
        base: bool,
        kind: Publication,
        params: P,
    ) -> Result<usize> {
        self.execute(
            &self.publications[usize::from(base) * 3 + kind as usize],
            params,
        )
    }

    fn execute<P: Params>(&self, query: &RefCell<Statement<'conn>>, params: P) -> Result<usize> {
        let mut statement = query
            .try_borrow_mut()
            .map_err(|_| Error::operation("prepared query is already active"))?;
        let before = statement.get_status(StatementStatus::MemUsed) as usize;
        let result = statement.execute(params);
        statement.clear_bindings();
        self.check(
            before,
            statement.get_status(StatementStatus::MemUsed) as usize,
        )?;
        Ok(result?)
    }

    fn check(&self, before: usize, after: usize) -> Result<()> {
        let bytes = self
            .bytes
            .get()
            .saturating_sub(before)
            .saturating_add(after);
        self.bytes.set(bytes);
        if bytes > self.limit {
            return Err(limit_error());
        }
        Ok(())
    }
}

fn limit_error() -> Error {
    Error::new(
        ErrorKind::Limit,
        "prepared lookup statements exceed the working-memory read budget",
    )
}
