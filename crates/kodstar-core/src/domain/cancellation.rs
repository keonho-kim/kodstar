// Purpose: 공유 취소 신호와 취소 오류 확인.
// Pattern: Cancellation Token.
// Usage: 분석·조회·잠금 대기가 전달받아 확인.
// Related: crates/kodstar-core/src/domain/diagnostics.rs.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::diagnostics::{Error, ErrorKind, Result};

#[derive(Clone, Debug, Default)]
pub struct Cancellation {
    cancelled: Arc<AtomicBool>,
    external: Option<&'static AtomicBool>,
}

impl Cancellation {
    /// Observe an explicitly supplied process-lifetime signal flag without owning a background thread.
    pub fn observing(flag: &'static AtomicBool) -> Self {
        Self {
            external: Some(flag),
            ..Self::default()
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
            || self
                .external
                .is_some_and(|flag| flag.load(Ordering::Acquire))
    }

    pub(crate) fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(Error::new(ErrorKind::Cancelled, "operation cancelled"))
        } else {
            Ok(())
        }
    }
}
