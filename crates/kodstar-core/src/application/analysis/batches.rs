// Purpose: 파일별 fact를 제한된 바이트·개수의 배치로 전달.
// Pattern: Bounded Batch.
// Usage: worker가 파일 추출 동안 소유하고 성공 시 finish로 나머지를 전달.
// Related: crates/kodstar-core/src/application/analysis/events.rs, crates/kodstar-core/src/application/analysis/workers.rs.

use super::events::{Event, Events};
use crate::domain::diagnostics::{Error, ErrorKind, Result};
use crate::domain::facts::Fact;
use crate::domain::languages::Language;
use std::path::PathBuf;
use std::sync::Arc;

pub(super) struct BatchSender<'a> {
    events: &'a Events,
    path: Arc<PathBuf>,
    pending: Vec<(Language, Fact)>,
    bytes: usize,
    target: usize,
    fact_limit: usize,
}

impl<'a> BatchSender<'a> {
    pub(super) fn new(events: &'a Events, path: Arc<PathBuf>, fact_limit: usize) -> Self {
        Self {
            events,
            path,
            pending: Vec::new(),
            bytes: 0,
            target: fact_limit.min(64 * 1024),
            fact_limit,
        }
    }

    pub(super) fn push(&mut self, language: Language, fact: Fact) -> Result<()> {
        self.events.cancel.check()?;
        let bytes = fact.owned_bytes();
        if bytes > self.fact_limit {
            return Err(Error::new(
                ErrorKind::Limit,
                "one syntax fact exceeds fact_batch_bytes",
            ));
        }
        let extra = bytes - std::mem::size_of::<Fact>();
        if !self.pending.is_empty()
            && (self.pending.len() == self.pending.capacity() || self.bytes + extra > self.target)
        {
            self.flush()?;
        }
        if self.pending.capacity() == 0 {
            let capacity = (self.target.saturating_sub(extra)
                / std::mem::size_of::<(Language, Fact)>())
            .clamp(1, 128);
            self.pending = Vec::with_capacity(capacity);
            self.bytes = self.pending.capacity() * std::mem::size_of::<(Language, Fact)>();
        }
        self.bytes += extra;
        self.pending.push((language, fact));
        Ok(())
    }

    pub(super) fn finish(mut self) -> Result<()> {
        self.flush()
    }

    fn flush(&mut self) -> Result<()> {
        if !self.pending.is_empty() {
            self.events.send(Event::Facts {
                path: Arc::clone(&self.path),
                facts: std::mem::take(&mut self.pending),
            })?;
            self.bytes = 0;
        }
        Ok(())
    }
}
