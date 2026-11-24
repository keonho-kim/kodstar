// Purpose: 단일 native worker의 문맥과 분석 stack 예약 소유.
// Pattern: Resource Scope.
// Usage: pipeline이 worker 역할에 맞춰 stack과 native 문맥을 배정.
// Related: crates/kodstar-core/src/application/analysis/budget.rs, crates/kodstar-core/src/application/analysis/workers.rs.

use crate::{
    application::workspace::SelectedProject, domain::diagnostics::Result, infrastructure::clang,
};
use std::sync::Arc;

pub(in crate::application) const WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;
pub(in crate::application) const GENERAL_STACK_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub(super) struct Resources {
    pub native: Option<clang::Configuration>,
    pub bytes: usize,
}

impl Resources {
    pub fn new(project: &SelectedProject) -> Result<Self> {
        let bytes = project.settings.native_phase_bytes()?;
        Ok(Self {
            native: project
                .snapshot
                .clone()
                .map(|snapshot| clang::Configuration {
                    snapshot,
                    root: project.root.clone(),
                    max_file: project.settings.max_file_bytes,
                    budget: bytes,
                    headers: Arc::new(kodstar_clang::HeaderDecoder::default()),
                    excludes: project.settings.exclude.clone(),
                }),
            bytes,
        })
    }
}
