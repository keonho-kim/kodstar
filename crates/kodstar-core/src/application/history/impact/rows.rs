// Purpose: 두 그래프의 영향 결과와 전체 출력·방문 예산 조정.
// Pattern: Streaming Adapter.
// Usage: 변경 영향 use case가 이전·현재 그래프를 순서대로 전달.
// Related: crates/kodstar-core/src/application/history/impact.rs.

use crate::application::{
    query,
    workspace::{SelectedProject, Settings},
};
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
    history::HistoryRow,
    query::{Limit, Query, QueryOptions},
    results::{QueryMetadata, QueryRow, QuerySink, QuerySummary},
};
use rusqlite::Connection;
use std::num::NonZeroUsize;

pub(super) struct Output<'a> {
    options: QueryOptions,
    limit: Option<usize>,
    nodes: Option<usize>,
    cancel: &'a Cancellation,
    sink: &'a mut dyn QuerySink,
    summary: QuerySummary,
}

impl<'a> Output<'a> {
    pub fn new(
        options: &QueryOptions,
        settings: &Settings,
        cancel: &'a Cancellation,
        sink: &'a mut dyn QuerySink,
    ) -> Self {
        Self {
            options: options.clone(),
            limit: options.limit.resolve(settings.limit),
            nodes: options.max_nodes.resolve(settings.max_nodes),
            cancel,
            sink,
            summary: QuerySummary {
                complete: true,
                ..QuerySummary::default()
            },
        }
    }
    pub fn emit(
        &mut self,
        connection: &Connection,
        selected: &SelectedProject,
        checkpoint: &str,
    ) -> Result<()> {
        if self
            .limit
            .is_some_and(|limit| self.summary.result_count >= limit as u64)
            || self
                .nodes
                .is_some_and(|limit| self.summary.visited_count >= limit as u64)
        {
            self.summary.complete = false;
            self.summary.truncated = true;
            self.summary.reasons.push("comparison_budget");
            return Ok(());
        }
        let mut options = self.options.clone();
        options.revision = None;
        options.limit = self
            .limit
            .and_then(|v| NonZeroUsize::new(v - self.summary.result_count as usize))
            .map_or(Limit::Unlimited, Limit::Count);
        options.max_nodes = self
            .nodes
            .and_then(|v| NonZeroUsize::new(v - self.summary.visited_count as usize))
            .map_or(Limit::Unlimited, Limit::Count);
        let mut relay = Relay {
            checkpoint,
            sink: self.sink,
        };
        let summary = query::run(
            connection,
            query::ReadScope {
                selected,
                maintenance: None,
                repository: None,
            },
            &Query::Changes {
                since: "comparison".into(),
            },
            &options,
            self.cancel,
            &mut relay,
        )?;
        self.summary.result_count += summary.result_count;
        self.summary.visited_count += summary.visited_count;
        self.summary.unresolved_count += summary.unresolved_count;
        self.summary.complete &= summary.complete;
        self.summary.truncated |= summary.truncated;
        for reason in summary.reasons {
            if !self.summary.reasons.contains(&reason) {
                self.summary.reasons.push(reason);
            }
        }
        Ok(())
    }
    pub fn finish(self) -> Result<QuerySummary> {
        self.sink.finish(&self.summary)?;
        Ok(self.summary)
    }
}

struct Relay<'a> {
    checkpoint: &'a str,
    sink: &'a mut dyn QuerySink,
}
impl QuerySink for Relay<'_> {
    fn begin(&mut self, _: &QueryMetadata) -> Result<()> {
        Ok(())
    }
    fn finish(&mut self, _: &QuerySummary) -> Result<()> {
        Ok(())
    }
    fn row(&mut self, row: &QueryRow) -> Result<()> {
        if let QueryRow::Relationship(relationship) = row {
            self.sink.row(&QueryRow::History(HistoryRow::Impact {
                checkpoint: self.checkpoint.into(),
                relationship: relationship.clone(),
            }))
        } else {
            Err(Error::operation("unexpected comparison output row"))
        }
    }
}
