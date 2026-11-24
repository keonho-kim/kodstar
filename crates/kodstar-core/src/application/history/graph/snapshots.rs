// Purpose: 그래프 비교가 요구하는 분석 세대를 선택하고 부족한 세대를 준비.
// Pattern: Snapshot Coordinator.
// Usage: graph 비교가 저장소와 cache writer를 획득한 상태에서 호출.
// Related: crates/kodstar-core/src/application/history/graph.rs, crates/kodstar-core/src/application/analysis/mod.rs.

use std::time::Instant;

use rusqlite::{Connection, OptionalExtension, backup, params};

use super::{cache, edges};
use crate::application::{analysis, workspace::SelectedProject};
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Coverage, Error, ErrorKind, Result},
    relations::RELATION_REGISTRY_VERSION,
    results::QuerySink,
};
use crate::infrastructure::{
    cache::{CacheStore, ProjectCache},
    history::{Repository, checkpoints},
    index,
};

pub(super) struct Build<'a> {
    pub store: &'a CacheStore,
    pub cache: &'a ProjectCache,
    pub repository: &'a Repository,
    pub repository_id: &'a str,
    pub configuration: &'a str,
    pub cancel: &'a Cancellation,
}

struct Snapshot {
    environment: (String, u32, u32),
    coverage: Coverage,
    generation: i64,
    indexed_at: u64,
}

impl Build<'_> {
    pub(super) fn compare(
        &self,
        project: &mut SelectedProject,
        (from, to): (Option<i64>, i64),
        mut spool: Option<&mut edges::Spool>,
        sink: &mut dyn QuerySink,
    ) -> Result<cache::Comparison> {
        let path = self.cache.directory.join("past.sqlite");
        let mut current = self.current(project, from, to);
        let mut read = |id, side| {
            let evidence = spool.as_deref_mut().map(|spool| (spool, side));
            if let Some((connection, checkpoint)) = &mut current
                && *checkpoint == checkpoints::display(id)
            {
                return collect(
                    connection,
                    evidence,
                    project.settings.memory_budget,
                    self.cancel,
                );
            }
            let mut connection =
                index::open(&path, &project.root, true, project.settings.db_cache_bytes)?;
            if let Some((source, _)) = &current
                && connection.query_row("SELECT generation=0 FROM metadata", [], |row| {
                    row.get::<_, bool>(0)
                })?
            {
                seed(source, &mut connection, self.cancel)?;
            }
            attach(project, self.repository, id)?;
            analysis::run(
                analysis::Session {
                    store: self.store,
                    selected: project,
                    cache: self.cache,
                    database: &path,
                    started: Instant::now(),
                },
                &mut connection,
                analysis::InitOptions::default(),
                self.cancel,
                &mut |event| sink.progress(event),
            )?;
            collect(
                &mut connection,
                evidence,
                project.settings.memory_budget,
                self.cancel,
            )
        };
        let before = from
            .filter(|id| *id != to)
            .map(|id| read(id, 0))
            .transpose()?;
        let after = read(to, 1)?;
        if before
            .as_ref()
            .is_some_and(|old| old.environment != after.environment)
        {
            return Err(Error::operation("comparison analysis environments differ"));
        }
        Ok(cache::Comparison {
            generation: after.generation,
            indexed_at: after.indexed_at,
            before: if from == Some(to) {
                Some(after.coverage.clone())
            } else {
                before.map(|old| old.coverage)
            },
            after: after.coverage,
        })
    }

    fn current(
        &self,
        project: &SelectedProject,
        from: Option<i64>,
        to: i64,
    ) -> Option<(Connection, String)> {
        let result = (|| -> Result<_> {
            let connection = index::open(
                &self.cache.directory.join("index.sqlite"),
                &project.root,
                false,
                project.settings.db_cache_bytes,
            )?;
            let checkpoint: Option<String> = connection
                .query_row(
                    "SELECT checkpoint FROM metadata
                 WHERE generation>0 AND last_update='complete' AND last_error IS NULL
                   AND repository=?1 AND config_hash=?2 AND extraction_version=?3
                   AND relation_registry_version=?4 AND checkpoint IN (?5,?6)",
                    params![
                        self.repository_id,
                        self.configuration,
                        index::EXTRACTION_VERSION,
                        RELATION_REGISTRY_VERSION,
                        from.map(checkpoints::display),
                        checkpoints::display(to)
                    ],
                    |row| row.get(0),
                )
                .optional()?;
            Ok(checkpoint.map(|checkpoint| (connection, checkpoint)))
        })();
        match result {
            Ok(source) => source,
            Err(error) => {
                if error.kind != ErrorKind::NotFound {
                    tracing::info!(kind = ?error.kind, "current analysis cannot be reused for graph comparison");
                }
                None
            }
        }
    }
}

fn seed(source: &Connection, target: &mut Connection, cancel: &Cancellation) -> Result<()> {
    let backup = backup::Backup::new(source, target)?;
    loop {
        cancel.check()?;
        match backup.step(256)? {
            backup::StepResult::Done => return Ok(()),
            backup::StepResult::More => {}
            _ => {
                return Err(Error::operation(
                    "could not copy the analysis into an empty history cache",
                ));
            }
        }
    }
}

pub(super) fn attach(
    project: &mut SelectedProject,
    repository: &Repository,
    id: i64,
) -> Result<()> {
    project.snapshot = Some(repository.snapshot(id));
    if let Some(state) = &mut project.history {
        state.checkpoint = checkpoints::display(id);
        state.analysis_checkpoint = Some(state.checkpoint.clone());
        state.tracked_files = repository.connection.query_row(
            "SELECT tracked_files FROM checkpoints WHERE id=?1",
            [id],
            |row| row.get(0),
        )?;
    }
    Ok(())
}

fn collect(
    connection: &mut Connection,
    spool: Option<(&mut edges::Spool, i64)>,
    budget: usize,
    cancel: &Cancellation,
) -> Result<Snapshot> {
    index::limit_reads(connection, budget)?;
    index::observe_cancellation(connection, cancel)?;
    let transaction = connection.transaction()?;
    let result = (|| {
        let environment = transaction.query_row(
            "SELECT config_hash,extraction_version,relation_registry_version FROM metadata",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let (generation, indexed_at) = index::require_committed(&transaction)?;
        let coverage = index::coverage(&transaction)?;
        if let Some((spool, side)) = spool {
            spool.capture(&transaction, side, cancel)?;
        }
        Ok(Snapshot {
            environment,
            coverage,
            generation,
            indexed_at,
        })
    })();
    index::stop_observing(&transaction)?;
    transaction.finish()?;
    result
}
