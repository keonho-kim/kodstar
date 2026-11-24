// Purpose: 저장소의 라이선스 원문을 바이너리용 압축 자산으로 생성.
// Pattern: Resource Compiler.
// Usage: Cargo가 CLI를 빌드할 때 OUT_DIR에만 기록.
// Related: crates/kodstar-cli/src/output/licenses.rs, LICENSES/third-party-notices.txt.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = "../../LICENSES/third-party-notices.txt";
    println!("cargo:rerun-if-changed={source}");
    let bytes = std::fs::read(source)?;
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?);
    std::fs::write(output.join("notices.zst"), zstd::bulk::compress(&bytes, 9)?)?;
    Ok(())
}
