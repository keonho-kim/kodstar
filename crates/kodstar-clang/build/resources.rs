// Purpose: 내장 표준 C/C++ 헤더의 고정 build 입력과 대상별 경로 선택.
// Pattern: Resource Profile.
// Usage: header pack 생성기가 builtin·libc++·Darwin 원문 루트를 요청.
// Related: crates/kodstar-clang/build/headers.rs, crates/kodstar-clang/build/toolchain.rs.

use super::toolchain::Llvm;
use std::path::PathBuf;

pub(super) fn verify_system(identity: &str) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("CARGO_CFG_TARGET_OS")? == "macos"
        && identity != "08147105c1fa85023262c776f59cc4672d4f01b25f59ee6248eaec6f158cbe38"
    {
        return Err("Darwin resource inputs differ from the pinned Zig 0.17.0 source tree".into());
    }
    Ok(())
}

pub(super) fn roots(
    llvm: &Llvm,
) -> Result<Vec<(&'static str, PathBuf)>, Box<dyn std::error::Error>> {
    let mut roots = vec![("include", llvm.resource.clone())];
    let cxx = llvm.include.join("c++/v1");
    if !cxx.join("__config_site").is_file() {
        return Err("matching libc++ headers and generated __config_site are required".into());
    }
    roots.push(("cxx", cxx));
    println!("cargo:rerun-if-env-changed=KODSTAR_ZIG_LIB_PATH");
    if std::env::var("CARGO_CFG_TARGET_OS")? == "macos" {
        let zig = std::env::var_os("KODSTAR_ZIG_LIB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/opt/homebrew/Cellar/zig/0.17.0/lib/zig"));
        let headers = zig.join("libc/include/any-darwin-any");
        if !headers.join("stdio.h").is_file() {
            return Err(
                "Zig 0.17.0 Darwin header inputs required; set KODSTAR_ZIG_LIB_PATH".into(),
            );
        }
        roots.push(("system", headers));
    }
    Ok(roots)
}
