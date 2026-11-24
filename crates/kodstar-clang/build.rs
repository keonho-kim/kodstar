// Purpose: 고정 Clang 정적 frontend와 메모리 header 리소스의 빌드 조정.
// Pattern: Composition Root.
// Usage: Cargo가 kodstar-clang을 빌드할 때 실행.
// Related: crates/kodstar-clang/build/toolchain.rs, crates/kodstar-clang/build/headers.rs.

#[path = "build/archives.rs"]
mod archives;
#[path = "build/compression.rs"]
mod compression;
#[path = "build/headers.rs"]
mod headers;
#[path = "build/resources.rs"]
mod resources;
#[path = "build/toolchain.rs"]
mod toolchain;

fn main() {
    if let Err(error) = build() {
        panic!("build embedded Clang: {error}");
    }
}

fn build() -> Result<(), Box<dyn std::error::Error>> {
    let llvm = toolchain::Llvm::locate()?;
    headers::generate(&llvm)?;
    toolchain::compile(&llvm)?;
    Ok(())
}
