// Purpose: 분석 transaction과 정상 세대의 원자적 공개.
// Pattern: Coordinator.
// Usage: Engine init·자동 refresh에서 호출.
// Related: crates/kodstar-core/src/application/execution/context.rs, crates/kodstar-core/src/application/analysis/pipeline.rs.

//! Atomic publication of one analysis generation.
mod batches;
mod budget;
mod configuration_input;
mod configurations;
mod discovery;
mod dotnet_patterns;
mod dotnet_projects;
mod events;
mod extraction;
mod incremental;
mod module_identity;
mod pipeline;
mod resolve;
mod swift_projects;
mod workers;

use super::workspace::SelectedProject;
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use crate::domain::results::{InitReport, Progress};
use crate::infrastructure::cache::{CacheStore, ProjectCache, now_seconds};
use crate::infrastructure::index;
use rusqlite::params;
use std::time::Instant;

#[derive(Clone, Debug, Default)]
pub struct InitOptions {
    pub path: Option<std::path::PathBuf>,
    pub full: bool,
    pub verify: bool,
}

pub(crate) struct Session<'a> {
    pub store: &'a CacheStore,
    pub selected: &'a SelectedProject,
    pub cache: &'a ProjectCache,
    pub database: &'a std::path::Path,
    pub started: Instant,
}

pub(crate) fn run(
    session: Session<'_>,
    connection: &mut rusqlite::Connection,
    options: InitOptions,
    cancel: &Cancellation,
    progress: &mut dyn FnMut(&Progress) -> Result<()>,
) -> Result<InitReport> {
    let Session {
        store,
        selected,
        cache,
        database,
        started: start,
    } = session;
    let previous_repository: Option<String> =
        connection.query_row("SELECT repository FROM metadata", [], |r| r.get(0))?;
    let (generation, previous_hash, previous_version, indexed_at,previous_registry): (i64,String,u32,u64,u32) = connection.query_row(
        "SELECT generation,config_hash,extraction_version,indexed_at,relation_registry_version FROM metadata WHERE singleton=1", [],
        |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)))?;
    let maintenance = super::maintenance::run(store, selected, now_seconds()?, cancel)?;
    let fingerprint = selected.settings.fingerprint()?;
    let full = options.full
        || previous_repository.as_deref()
            != selected.history.as_ref().map(|s| s.repository.as_str())
        || previous_hash != fingerprint
        || previous_version != index::EXTRACTION_VERSION
        || previous_registry != crate::domain::relations::RELATION_REGISTRY_VERSION;
    let next_generation = generation
        .checked_add(1)
        .ok_or_else(|| Error::operation("index generation overflow"))?;
    let reusable = !full
        && !options.verify
        && generation > 0
        && selected.history.is_some()
        && connection.query_row(
            "SELECT checkpoint IS ?1 AND last_update='complete' AND last_error IS NULL FROM metadata WHERE singleton=1",
            [selected.history.as_ref().map(|state| state.checkpoint.as_str())],
            |row| row.get::<_, bool>(0),
        )?;
    let (changed, parsed, reused, deleted, at) = if reusable {
        cancel.check()?;
        (
            false,
            0,
            index::coverage(connection)?.scanned_files,
            0,
            indexed_at,
        )
    } else {
        index::observe_cancellation(connection, cancel)?;
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let outcome: Result<(bool, u64, u64, u64, u64)> = (|| {
            transaction.execute_batch("CREATE TEMP TABLE IF NOT EXISTS seen_files(path BLOB PRIMARY KEY); CREATE TEMP TABLE IF NOT EXISTS changed_files(id INTEGER PRIMARY KEY);
                CREATE TEMP TABLE IF NOT EXISTS dependent_files(id INTEGER PRIMARY KEY);
                DELETE FROM seen_files; DELETE FROM changed_files; DELETE FROM dependent_files;")?;
            let (mut changed, mut parsed, reused, mut resolution) = pipeline::run(
                pipeline::Input {
                    database,
                    selected,
                    full,
                    verify: options.verify,
                    generation: next_generation,
                },
                &transaction,
                cancel,
                progress,
            )?;
            let deleted: u64 = transaction.query_row(
                "SELECT COUNT(*) FROM files WHERE path NOT IN (SELECT path FROM seen_files)",
                [],
                |row| row.get(0),
            )?;
            let semantic_deletions =
                deleted > 0 && index::inert_inputs::semantic_deletions(&transaction)?;
            let scoped_deletions =
                semantic_deletions && !full && index::invalidation::capture_deleted(&transaction)?;
            if scoped_deletions {
                resolution = resolution.max(incremental::Resolution::ChangedFiles);
            }
            if deleted > 0 {
                changed = true;
                transaction.execute(
                    "DELETE FROM files WHERE path NOT IN (SELECT path FROM seen_files)",
                    [],
                )?;
            }
            let checkpoint = selected
                .history
                .as_ref()
                .map(|state| state.checkpoint.as_str());
            let previous_checkpoint: Option<String> = transaction.query_row(
                "SELECT checkpoint FROM metadata WHERE singleton=1",
                [],
                |r| r.get(0),
            )?;
            changed |= full || previous_checkpoint.as_deref() != checkpoint;
            index::limit_reads(&transaction, selected.settings.memory_budget)?;
            let (configuration_changed, configuration_parsed) =
                configurations::run(selected, &transaction, next_generation, cancel, progress)?;
            changed |= configuration_changed;
            if full || (semantic_deletions && !scoped_deletions) || configuration_changed {
                resolution = incremental::Resolution::All;
            }
            parsed += configuration_parsed;
            let at = if changed {
                if resolution != incremental::Resolution::None {
                    progress(&Progress {
                        phase: "resolve",
                        completed_files: parsed + reused,
                        source_bytes: 0,
                    })?;
                }
                index::limit_reads(&transaction, selected.settings.memory_budget)?;
                if resolution == incremental::Resolution::All {
                    dotnet_projects::run(&transaction, &selected.root, cancel)?;
                    swift_projects::run(&transaction, &selected.root, cancel)?;
                }
                if resolution != incremental::Resolution::None {
                    resolve::run(&transaction, cancel, resolution)?;
                }
                index::refresh_coverage(&transaction, next_generation)?;
                let at = now_seconds()?;
                transaction.execute("UPDATE metadata SET generation=?1,config_hash=?2,indexed_at=?3,last_update='complete',last_error=NULL,engine_version=?4,extraction_version=?5,relation_registry_version=?6 WHERE singleton=1",params![next_generation,fingerprint,at,crate::VERSION,index::EXTRACTION_VERSION,crate::domain::relations::RELATION_REGISTRY_VERSION])?;
                at
            } else {
                transaction.execute(
                    "UPDATE metadata SET last_update='complete',last_error=NULL
                 WHERE singleton=1 AND (last_update<>'complete' OR last_error IS NOT NULL)",
                    [],
                )?;
                indexed_at
            };
            cancel.check()?;
            transaction.execute(
                "UPDATE metadata SET checkpoint=?1,repository=?2 WHERE singleton=1",
                params![
                    checkpoint,
                    selected.history.as_ref().map(|s| s.repository.as_str())
                ],
            )?;
            Ok((changed, parsed, reused, deleted, at))
        })();
        let outcome = match outcome {
            Ok(value) => value,
            Err(error) => {
                index::stop_observing(&transaction)?;
                transaction.finish().map_err(|rollback| {
                    Error::caused_by(
                        format!("analysis failed ({error}); rollback failed"),
                        rollback,
                    )
                })?;
                connection.execute(
                    "UPDATE metadata SET last_update=?1,last_error=?2 WHERE singleton=1",
                    params![
                        if error.kind == crate::domain::diagnostics::ErrorKind::Cancelled {
                            "cancelled"
                        } else {
                            "failed"
                        },
                        error.to_string()
                    ],
                )?;
                return Err(error);
            }
        };
        transaction.commit()?;
        index::stop_observing(connection)?;
        outcome
    };
    let mut coverage = index::coverage(connection)?;
    coverage.reused_files = reused;
    Ok(InitReport {
        history: selected.history.clone(),
        maintenance,
        root: selected.root.to_string_lossy().into_owned(),
        root_encoding: crate::infrastructure::cache::paths::encoding(&selected.root),
        cache_encoding: crate::infrastructure::cache::paths::encoding(&cache.directory),
        cache: cache.directory.to_string_lossy().into_owned(),
        generation: if changed { next_generation } else { generation },
        indexed_at: at,
        changed,
        parsed_files: parsed,
        deleted_files: deleted,
        workers: selected.settings.jobs,
        duration_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
        coverage,
    })
}
