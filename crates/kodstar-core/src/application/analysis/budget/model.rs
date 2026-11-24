// Purpose: Model actual byte-permit contention, release and wakeup within bounded schedules.
// Pattern: Concurrency Model Test.
// Usage: cargo test --lib selects this module with --cfg=kodstar_loom.
// Related: crates/kodstar-core/src/application/analysis/budget.rs, docs/development.md.

use super::*;
use loom::sync::atomic::{AtomicUsize, Ordering};

fn reservations(limit: usize, bytes: usize, workers: usize) {
    let mut model = loom::model::Builder::new();
    model.preemption_bound = Some(2);
    model.max_permutations = Some(10_000);
    model.check(move || {
        let budget = ByteBudget::new(limit).unwrap();
        let active = loom::sync::Arc::new(AtomicUsize::new(0));
        let mut threads = Vec::new();
        for _ in 0..workers {
            let budget = Arc::clone(&budget);
            let active = loom::sync::Arc::clone(&active);
            threads.push(loom::thread::spawn(move || {
                let permit = budget.reserve(bytes, &Cancellation::default()).unwrap();
                let before = active.fetch_add(bytes, Ordering::SeqCst);
                assert!(
                    before + bytes <= limit,
                    "admitted bytes exceeded the budget"
                );
                loom::thread::yield_now();
                assert!(active.fetch_sub(bytes, Ordering::SeqCst) >= bytes);
                drop(permit);
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert_eq!(*budget.used.lock().unwrap(), 0);
        let all = budget.reserve(limit, &Cancellation::default()).unwrap();
        drop(all);
    });
}

#[test]
fn a_full_reservation_wakes_the_next_owner_when_released() {
    reservations(1, 1, 2);
}

#[test]
fn simultaneous_small_leases_preserve_capacity_and_return_every_byte() {
    reservations(2, 1, 3);
}
