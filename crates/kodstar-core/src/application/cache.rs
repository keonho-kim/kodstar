// Purpose: cache 목록·명시적 정리·TTL·용량 정책의 실행.
// Pattern: Coordinator.
// Usage: Engine cache API와 자동 maintenance가 같은 정책을 호출.
// Related: crates/kodstar-core/src/application/maintenance.rs, crates/kodstar-core/src/infrastructure/cache/catalog.rs, crates/kodstar-core/src/infrastructure/cache/removal.rs.

use super::workspace::{self, ProjectOptions, SelectedProject, Selection, Settings};
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
    results::{CacheEntry, QueryMetadata, QueryRow, QuerySink, QuerySummary},
};
use crate::infrastructure::cache::{
    CacheStore,
    catalog::Catalog,
    now_seconds,
    paths::cache_key,
    removal::{self, Removal},
};
use std::path::Path;

#[derive(Clone, Copy, Debug)]
pub enum CacheRequest {
    List,
    Clean { all: bool },
    Gc { dry_run: bool },
}

pub(super) struct Policy<'a> {
    pub settings: &'a Settings,
    pub request: CacheRequest,
    pub only_root: Option<&'a Path>,
    pub protected: Option<&'a Path>,
    pub now: u64,
    pub deadline: Option<std::time::Instant>,
}

pub(crate) fn run(
    store: &CacheStore,
    project: &ProjectOptions,
    request: CacheRequest,
    cancel: &Cancellation,
    sink: &mut dyn QuerySink,
    prepared: Option<&SelectedProject>,
    metadata: QueryMetadata,
) -> Result<QuerySummary> {
    let explicit = project.config.as_ref().map(|path| project.cwd.join(path));
    let loaded;
    let settings = if let Some(selected) = prepared {
        &selected.settings
    } else {
        loaded = Settings::load(&store.home, explicit.as_deref(), &project.overrides)?;
        &loaded
    };
    let current = if matches!(request, CacheRequest::Clean { all: false }) {
        Some(workspace::select(store, project, Selection::Existing, cancel)?.root)
    } else {
        None
    };
    let catalog = Catalog::scan(store, cancel, None)?;
    let mut summary = QuerySummary {
        complete: true,
        ..QuerySummary::default()
    };
    sink.begin(&metadata)?;
    let outcome = collect(
        store,
        &catalog,
        Policy {
            settings,
            request,
            only_root: current.as_deref(),
            protected: None,
            deadline: None,
            now: now_seconds()?,
        },
        cancel,
        &mut summary,
        &mut |entry| sink.row(&QueryRow::Cache(entry)),
    );
    if outcome.is_err() {
        summary.complete = false;
        summary.reasons.push("cache_operation_failed");
    }
    sink.finish(&summary)?;
    outcome?;
    Ok(summary)
}

pub(super) fn collect(
    store: &CacheStore,
    catalog: &Catalog,
    policy: Policy<'_>,
    cancel: &Cancellation,
    summary: &mut QuerySummary,
    emit: &mut dyn FnMut(CacheEntry) -> Result<()>,
) -> Result<u64> {
    let ttl = policy
        .settings
        .cache_ttl_days
        .checked_mul(86400)
        .ok_or_else(|| Error::argument("cache_ttl_days overflows seconds"))?;
    let mut total = catalog.total_bytes()?;
    let protected = policy.protected.map(cache_key);
    catalog.visit(store, &mut |mut record| {
        cancel.check()?;
        if policy
            .only_root
            .is_some_and(|root| record.root.as_deref() != Some(root))
        {
            return Ok(());
        }
        summary.visited_count += 1;
        let reason = match policy.request {
            CacheRequest::List => None,
            CacheRequest::Clean { .. } => Some("explicit_cleanup"),
            CacheRequest::Gc { .. } if policy.now.saturating_sub(record.entry.last_used) >= ttl => {
                Some("expired")
            }
            CacheRequest::Gc { .. }
                if catalog.complete && total > policy.settings.cache_budget_bytes =>
            {
                Some("capacity")
            }
            _ => None,
        };
        if let Some(reason) = reason {
            if protected.as_deref() == Some(record.key.as_str()) {
                record.entry.reason = Some("current_project");
            } else if record.entry.active {
                record.entry.reason = Some("active");
            } else if !record.safe {
                summary.complete = false;
                if !summary.reasons.contains(&"protected_cache") {
                    summary.reasons.push("protected_cache");
                }
            } else if policy
                .deadline
                .is_some_and(|deadline| std::time::Instant::now() >= deadline)
            {
                record.entry.reason = Some("maintenance_time_limit");
                summary.complete = false;
                if !summary.reasons.contains(&"maintenance_time_limit") {
                    summary.reasons.push("maintenance_time_limit");
                }
            } else if let Some(root) = &record.root {
                if matches!(policy.request, CacheRequest::Gc { dry_run: true }) {
                    record.entry.action = "would_remove";
                    record.entry.reason = Some(reason);
                    total = total.saturating_sub(record.entry.bytes);
                } else {
                    match removal::remove(
                        store,
                        &record.key,
                        root,
                        matches!(policy.request, CacheRequest::Gc { .. })
                            .then_some(record.entry.last_used),
                        policy.deadline.is_none(),
                        cancel,
                    )? {
                        Removal::Removed => {
                            record.entry.action = "removed";
                            record.entry.reason = Some(reason);
                            total = total.saturating_sub(record.entry.bytes);
                        }
                        Removal::Protected(reason) => {
                            record.entry.active = reason == "active";
                            record.entry.reason = Some(reason);
                            if reason == "registry_busy" {
                                summary.complete = false;
                                if !summary.reasons.contains(&"maintenance_registry_busy") {
                                    summary.reasons.push("maintenance_registry_busy");
                                }
                            }
                        }
                    }
                }
            }
        }
        emit(record.entry)?;
        summary.result_count += 1;
        Ok(())
    })?;
    if matches!(policy.request, CacheRequest::Gc { .. }) {
        if total > policy.settings.cache_budget_bytes {
            summary.complete = false;
            summary.reasons.push("cache_budget_unmet");
        }
        if !catalog.complete {
            summary.complete = false;
            summary
                .reasons
                .push(catalog.stop_reason.unwrap_or("maintenance_scan_limit"));
        }
        if !catalog.size_complete()? {
            summary.complete = false;
            summary.reasons.push("cache_size_incomplete");
        }
    }
    Ok(total)
}
