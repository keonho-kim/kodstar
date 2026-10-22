// Purpose: 이력 명령의 공통 실행기 연결과 분석 독립 준비 정책.
// Pattern: Command Handler.
// Usage: Engine::history가 공유 validate·cancel 경계를 사용.
// Related: crates/kodstar-core/src/application/history/mod.rs.

use super::{
    CommandHandler,
    context::{Context, Plan},
};
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::Result,
    history::HistoryRequest,
    results::{QuerySink, QuerySummary},
};

pub(crate) struct History<'a> {
    pub request: &'a HistoryRequest,
    pub sink: &'a mut dyn QuerySink,
}
impl CommandHandler for History<'_> {
    type Output = QuerySummary;
    fn validate(&self) -> Result<()> {
        crate::application::history::validate(self.request)
    }
    fn plan(&self) -> Plan<'_> {
        Plan::Detached
    }
    fn execute(self, context: &mut Context<'_>, cancel: &Cancellation) -> Result<QuerySummary> {
        crate::application::history::run(
            context.store,
            context.options,
            self.request,
            cancel,
            self.sink,
        )
    }
}
