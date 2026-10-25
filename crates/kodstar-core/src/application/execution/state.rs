// Purpose: 명령 의도와 현재·watch·고정 revision의 유효한 실행 상태.
// Pattern: Hierarchical State.
// Usage: 공통 Context가 동기화와 observer 수명을 결정.
// Related: crates/kodstar-core/src/application/execution/context.rs, crates/kodstar-core/src/application/execution/watch.rs.

use crate::infrastructure::watch::Changes;

pub(super) enum Intent {
    Initialize { full: bool },
    Regular,
    Diagnostics,
}

impl Intent {
    pub fn initializes(&self) -> bool {
        matches!(self, Self::Initialize { .. })
    }
    pub fn full(&self) -> bool {
        matches!(self, Self::Initialize { full: true })
    }
    pub fn diagnostic(&self) -> bool {
        matches!(self, Self::Diagnostics)
    }
    pub fn synchronized(&mut self) {
        if self.initializes() {
            *self = Self::Regular;
        }
    }
}

pub(super) enum InputState {
    Current,
    Watching { observer: Option<Changes> },
    Revision,
}

impl InputState {
    pub fn historical(&self) -> bool {
        matches!(self, Self::Revision)
    }
    pub fn watching(&self) -> bool {
        matches!(self, Self::Watching { .. })
    }
    pub fn take_observer(&mut self) -> Option<Changes> {
        match self {
            Self::Watching { observer } => observer.take(),
            _ => None,
        }
    }
}
