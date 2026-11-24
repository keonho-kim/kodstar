// Purpose: 단일 바이너리에 포함된 제삼자 라이선스 원문과 header 고지를 출력.
// Pattern: Output Adapter.
// Usage: main이 --licenses 요청에 대해 프로젝트 선택 전에 호출.
// Related: crates/kodstar-cli/src/main.rs, LICENSES/third-party-notices.txt, crates/kodstar-clang/src/lib.rs.

use std::io::{self, Write};
use std::process::ExitCode;

pub(crate) fn render() -> ExitCode {
    let outcome = (|| -> io::Result<()> {
        let mut stdout = io::stdout().lock();
        stdout.write_all(b"Third-party notices\n\n")?;
        let notices = include_bytes!(concat!(env!("OUT_DIR"), "/notices.zst"));
        zstd::stream::copy_decode(notices.as_slice(), &mut stdout)?;
        zstd::stream::copy_decode(kodstar_clang::HEADER_NOTICES_ZSTD, &mut stdout)?;
        stdout.flush()
    })();
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => super::render_error(&super::write_error(error)),
    }
}
