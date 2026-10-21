// Purpose: 세대가 소유하는 고정 SQL 10개의 수명·메모리·매개변수 정리.
// Pattern: Simple Module.
// Usage: resolve가 생성하고 binding·후보·증거·관계 저장이 빌림.
// Related: crates/kodstar-core/src/application/analysis/resolve/mod.rs, crates/kodstar-core/src/application/analysis/resolve/publication.rs.

use crate::application::analysis::resolve::Binding;
use crate::domain::diagnostics::{Error, ErrorKind, Result};
use rusqlite::{Connection, OptionalExtension, Params, Statement, StatementStatus};
use std::cell::{Cell, RefCell};

#[derive(Clone, Copy)]
pub(super) enum Publication {
    Internal,
    External,
    Unresolved,
}

pub(super) struct Prepared<'conn> {
    binding: RefCell<Statement<'conn>>,
    candidates: RefCell<Statement<'conn>>,
    rust: RefCell<Statement<'conn>>,
    dependency: RefCell<Statement<'conn>>,
    publications: [RefCell<Statement<'conn>>; 6],
    bytes: Cell<usize>,
    limit: usize,
}

impl<'conn> Prepared<'conn> {
    pub(super) fn new(c: &'conn Connection) -> Result<Self> {
        let binding = RefCell::new(c.prepare(include_str!("prepared/bindings.sql"))?);
        let candidates = RefCell::new(c.prepare(include_str!("prepared/candidates.sql"))?);
        let rust = RefCell::new(c.prepare(include_str!("prepared/rust.sql"))?);
        let dependency=RefCell::new(c.prepare("INSERT OR REPLACE INTO resolution_dependencies(fact_id,kind,lookup_key,found) VALUES(?1,?2,?3,?4)")?);
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
        let bytes = [&binding, &candidates, &rust, &dependency]
            .into_iter()
            .chain(publications.iter())
            .map(|s| s.borrow().get_status(StatementStatus::MemUsed) as usize)
            .fold(0usize, usize::saturating_add);
        let limit = c.limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH)? as usize;
        if bytes > limit {
            return Err(limit_error());
        }
        Ok(Self {
            binding,
            candidates,
            rust,
            dependency,
            publications: publications
                .try_into()
                .map_err(|_| Error::operation("invalid fixed publication query count"))?,
            bytes: Cell::new(bytes),
            limit,
        })
    }

    pub(super) fn binding<P: Params>(&self, params: P) -> Result<Option<Binding>> {
        let mut statement = self
            .binding
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
        self.execute(&self.candidates, params)
    }
    pub(super) fn rust<P: Params>(&self, params: P) -> Result<usize> {
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
