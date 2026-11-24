// Purpose: CLI 상세 출력 설정에 따라 단계 계측과 subscriber 수명을 소유.
// Pattern: Composition Root.
// Usage: main이 --verbose 값을 전달하고 실행 전체에 guard를 유지.
// Related: crates/kodstar-cli/src/main.rs, crates/kodstar-cli/src/session/mod.rs.

use tracing::subscriber::DefaultGuard;
use tracing_subscriber::fmt::{format::FmtSpan, time::Uptime};

pub(crate) fn start(verbose: bool) -> Option<DefaultGuard> {
    if !verbose {
        return None;
    }
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_target(false)
        .with_timer(Uptime::default())
        .with_span_events(FmtSpan::CLOSE)
        .finish();
    Some(tracing::subscriber::set_default(subscriber))
}
