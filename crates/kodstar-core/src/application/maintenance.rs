// Purpose: 일반 분석·조회에서 제한된 자동 GC의 빈도와 현재 루트 보호.
// Pattern: Coordinator.
// Usage: 분석 실행과 no-refresh 조회가 실제 작업 시작 전에 호출.
// Related: crates/kodstar-core/src/application/cache.rs, crates/kodstar-core/src/infrastructure/cache/maintenance.rs.

use super::{
    cache::{self, CacheRequest, Policy},
    workspace::SelectedProject,
};
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, ErrorKind, Result},
    results::{MaintenanceReport, QuerySummary},
};
use crate::infrastructure::cache::{
    CacheStore,
    catalog::Catalog,
    inspection::ScanBudget,
    maintenance::{self, SavedState, StateLease},
};

pub(crate) fn run(
    store: &CacheStore,
    selected: &SelectedProject,
    now: u64,
    cancel: &Cancellation,
) -> Result<Option<MaintenanceReport>> {
    let Some(lease) = StateLease::acquire(store, cancel)? else {
        return Ok(None);
    };
    let policy = serde_json::to_vec(&(
        selected.settings.cache_ttl_days,
        selected.settings.cache_budget_bytes,
        selected.settings.auto_gc_interval_seconds,
    ))
    .map_err(|error| Error::caused_by("encode cache lifetime policy", error))?;
    let hash = blake3::hash(&policy).to_hex().to_string();
    if let Some(mut state) = maintenance::read(&store.home)? {
        if state.report.status == "running" {
            state.report.status = "interrupted".into();
            state.report.complete = false;
            state.report.reason = Some("previous automatic GC did not publish completion".into());
            lease.write(&state)?;
        }
        if state.policy_hash == hash
            && state.report.last_run_at <= now
            && now - state.report.last_run_at < selected.settings.auto_gc_interval_seconds
        {
            return Ok(Some(state.report));
        }
    }
    let mut state = SavedState::new(
        hash,
        MaintenanceReport {
            last_run_at: now,
            status: "running".into(),
            complete: false,
            visited: 0,
            removed: 0,
            remaining_bytes: 0,
            size_complete: false,
            reason: None,
        },
    );
    lease.write(&state)?;
    let outcome: Result<()> = (|| {
        let mut budget = ScanBudget::new();
        let catalog = Catalog::scan(store, cancel, Some(&mut budget))?;
        let mut summary = QuerySummary {
            complete: true,
            ..QuerySummary::default()
        };
        let remaining = cache::collect(
            store,
            &catalog,
            Policy {
                settings: &selected.settings,
                request: CacheRequest::Gc { dry_run: false },
                only_root: None,
                protected: Some(&selected.root),
                now,
                deadline: Some(budget.deadline()),
            },
            cancel,
            &mut summary,
            &mut |entry| {
                if entry.action == "removed" {
                    state.report.removed += 1;
                }
                Ok(())
            },
        )?;
        state.report.complete = summary.complete;
        state.report.status = if summary.complete {
            "complete"
        } else {
            "partial"
        }
        .into();
        state.report.visited = summary.visited_count;
        state.report.remaining_bytes = remaining;
        state.report.size_complete = catalog.complete && catalog.size_complete()?;
        state.report.reason = (!summary.reasons.is_empty()).then(|| summary.reasons.join(","));
        Ok(())
    })();
    if let Err(error) = &outcome {
        state.report.status = if error.kind == ErrorKind::Cancelled {
            "cancelled"
        } else {
            "failed"
        }
        .into();
        let message = error.to_string();
        state.report.reason = Some(message.chars().take(4096).collect());
    }
    let persisted = lease.write(&state);
    match (outcome, persisted) {
        (Err(error), Err(write)) => {
            let kind = error.kind;
            let mut error = Error::caused_by(
                format!("automatic GC failed; final state could not be recorded ({write})"),
                error,
            );
            error.kind = kind;
            Err(error)
        }
        (Err(error), _) | (_, Err(error)) => Err(error),
        (Ok(()), Ok(())) => Ok(Some(state.report)),
    }
}
