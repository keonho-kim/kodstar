// Purpose: 프로세스 취소 신호를 명시적인 엔진 취소 값으로 연결.
// Pattern: RAII Signal Guard.
// Usage: CLI 실행 경계가 guard를 소유하고 작업 종료 시 해제.
// Related: crates/kodstar-cli/src/run.rs, crates/kodstar-core/src/domain/cancellation.rs.

use kodstar_core::{Cancellation, Error, Result};
use std::sync::atomic::{AtomicBool, Ordering};

// OS callbacks cannot borrow a request. This process-lifetime flag is their only shared state.
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

pub(crate) fn cancellation() -> Cancellation {
    Cancellation::observing(&INTERRUPTED)
}

#[cfg(unix)]
pub(crate) struct SignalGuard {
    previous: nix::sys::signal::SigAction,
}

#[cfg(unix)]
extern "C" fn interrupt(_: i32) {
    INTERRUPTED.store(true, Ordering::Release);
}

#[cfg(unix)]
impl SignalGuard {
    pub(crate) fn install() -> Result<Self> {
        use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};
        INTERRUPTED.store(false, Ordering::Release);
        let action = SigAction::new(
            SigHandler::Handler(interrupt),
            SaFlags::empty(),
            SigSet::empty(),
        );
        // SAFETY: the static callback only stores a lock-free atomic flag; it never allocates or borrows request state.
        let previous = unsafe { sigaction(Signal::SIGINT, &action) }
            .map_err(|error| Error::caused_by("install interrupt handler", error))?;
        Ok(Self { previous })
    }
}

#[cfg(unix)]
impl Drop for SignalGuard {
    fn drop(&mut self) {
        // SAFETY: restore the action returned for the same valid signal. Drop is best-effort; the process is exiting.
        let _ = unsafe {
            nix::sys::signal::sigaction(nix::sys::signal::Signal::SIGINT, &self.previous)
        };
    }
}

#[cfg(windows)]
pub(crate) struct SignalGuard;

#[cfg(windows)]
unsafe extern "system" fn interrupt(event: u32) -> i32 {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT};
    if event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT {
        INTERRUPTED.store(true, Ordering::Release);
        1
    } else {
        0
    }
}

#[cfg(windows)]
impl SignalGuard {
    pub(crate) fn install() -> Result<Self> {
        use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
        INTERRUPTED.store(false, Ordering::Release);
        // SAFETY: this process-lifetime callback uses only the static atomic and the documented event value.
        if unsafe { SetConsoleCtrlHandler(Some(interrupt), 1) } == 0 {
            return Err(Error::caused_by(
                "install console interrupt handler",
                std::io::Error::last_os_error(),
            ));
        }
        Ok(Self)
    }
}

#[cfg(windows)]
impl Drop for SignalGuard {
    fn drop(&mut self) {
        // SAFETY: remove the same static callback registered by this guard; it owns no freed callback data.
        let _ = unsafe {
            windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(interrupt), 0)
        };
    }
}
