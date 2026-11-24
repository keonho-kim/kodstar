// Purpose: 바이트 예산 예약과 permit 반납.
// Pattern: RAII Permit.
// Usage: 입력과 이벤트 소유자가 예약·해제.
// Related: crates/kodstar-core/src/application/analysis/events.rs, crates/kodstar-core/src/application/analysis/discovery.rs, crates/kodstar-core/src/application/analysis/budget/model.rs.

//! Byte permits account for owned input and queued facts until their consumer releases them.

#[cfg(all(test, kodstar_loom))]
use loom::sync::{Condvar, Mutex};
use std::sync::Arc;
#[cfg(not(all(test, kodstar_loom)))]
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, ErrorKind, Result};

#[derive(Debug)]
pub(crate) struct ByteBudget {
    limit: usize,
    used: Mutex<usize>,
    available: Condvar,
}

impl ByteBudget {
    pub(crate) fn new(limit: usize) -> Result<Arc<Self>> {
        if limit == 0 {
            return Err(Error::argument("byte budget must be greater than zero"));
        }
        Ok(Arc::new(Self {
            limit,
            used: Mutex::new(0),
            available: Condvar::new(),
        }))
    }

    pub(crate) fn reserve(self: &Arc<Self>, bytes: usize, cancel: &Cancellation) -> Result<Permit> {
        if bytes > self.limit {
            return Err(Error::new(
                ErrorKind::Limit,
                format!(
                    "item requires {bytes} bytes, exceeding the {} byte queue budget",
                    self.limit,
                ),
            ));
        }
        let mut used = self
            .used
            .lock()
            .map_err(|_| Error::operation("byte budget lock poisoned"))?;
        loop {
            cancel.check()?;
            if bytes <= self.limit - *used {
                *used += bytes;
                return Ok(Permit {
                    budget: Arc::clone(self),
                    bytes,
                });
            }
            // A cancelled producer must wake even when no consumer releases a permit.
            used = self
                .available
                .wait_timeout(used, Duration::from_millis(25))
                .map_err(|_| Error::operation("byte budget lock poisoned"))?
                .0;
        }
    }
}

#[derive(Debug)]
pub(crate) struct Permit {
    budget: Arc<ByteBudget>,
    bytes: usize,
}

impl Drop for Permit {
    fn drop(&mut self) {
        if let Ok(mut used) = self.budget.used.lock() {
            *used -= self.bytes;
        }
        self.budget.available.notify_all();
    }
}

#[cfg(all(test, kodstar_loom))]
mod model;

#[cfg(all(test, not(kodstar_loom)))]
mod tests {
    use std::sync::mpsc;
    use std::thread;

    use super::*;

    #[test]
    fn bytes_remain_reserved_until_the_owner_drops() -> Result<()> {
        let budget = ByteBudget::new(8)?;
        let cancel = Cancellation::default();
        let first = budget.reserve(8, &cancel)?;
        assert_eq!(
            budget.reserve(9, &cancel).unwrap_err().kind,
            ErrorKind::Limit
        );
        drop(first);
        let _second = budget.reserve(8, &cancel)?;
        Ok(())
    }

    #[test]
    fn cancellation_terminates_a_waiting_producer() -> Result<()> {
        let budget = ByteBudget::new(8)?;
        let cancel = Cancellation::default();
        let _owner = budget.reserve(8, &cancel)?;
        let worker_budget = Arc::clone(&budget);
        let worker_cancel = cancel.clone();
        let (started, ready) = mpsc::channel();
        let worker = thread::spawn(move || {
            started
                .send(())
                .map_err(|error| Error::caused_by("signal test start", error))?;
            worker_budget.reserve(1, &worker_cancel)
        });
        ready
            .recv()
            .map_err(|error| Error::caused_by("wait for test producer", error))?;
        cancel.cancel();
        let result = worker
            .join()
            .map_err(|_| Error::operation("producer panicked"))?;
        assert_eq!(result.unwrap_err().kind, ErrorKind::Cancelled);
        Ok(())
    }
}
