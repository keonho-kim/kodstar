// Purpose: LLVM 23.1.4 위치·정적 archive·native bridge 컴파일 계약.
// Pattern: Build Adapter.
// Usage: build.rs가 검증한 toolchain으로만 native 코드를 연결.
// Related: crates/kodstar-clang/build.rs, crates/kodstar-clang/native/frontend.cpp.

use std::{
    path::{Path, PathBuf},
    process::Command,
};
pub(super) const VERSION: &str = "23.1.4";
pub(super) struct Llvm {
    pub include: PathBuf,
    pub lib: PathBuf,
    pub config: PathBuf,
    pub resource: PathBuf,
}

fn output(program: &Path, arguments: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new(program).args(arguments).output()?;
    if !output.status.success() {
        return Err(format!("{} {:?} failed", program.display(), arguments).into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().into())
}

impl Llvm {
    pub fn locate() -> Result<Self, Box<dyn std::error::Error>> {
        println!("cargo:rerun-if-env-changed=LLVM_CONFIG_PATH");
        let config = std::env::var_os("LLVM_CONFIG_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let brew = PathBuf::from("/opt/homebrew/opt/llvm/bin/llvm-config");
                if brew.is_file() {
                    brew
                } else {
                    PathBuf::from("llvm-config")
                }
            });
        let version = output(&config, &["--version"])?;
        if version != VERSION {
            return Err(format!("LLVM {VERSION} development archives required; found {version}. Set LLVM_CONFIG_PATH to the matching llvm-config").into());
        }
        let include = PathBuf::from(output(&config, &["--includedir"])?);
        let lib = PathBuf::from(output(&config, &["--libdir"])?);
        let resource = lib.join("clang/23/include");
        println!(
            "cargo:rustc-env=KODSTAR_CLANG_TARGET={}",
            output(&config, &["--host-target"])?
        );
        Ok(Self {
            include,
            lib,
            config,
            resource,
        })
    }
}

pub(super) fn compile(llvm: &Llvm) -> Result<(), Box<dyn std::error::Error>> {
    let mut identity = blake3::Hasher::new();
    identity.update(VERSION.as_bytes());
    identity.update(b"archive-digests-v1");
    for file in [
        "bridge.h",
        "budget.h",
        "frontend.cpp",
        "filesystem.cpp",
        "visitor.cpp",
    ] {
        identity.update(&std::fs::read(format!("native/{file}"))?);
    }
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .include(&llvm.include)
        .include("native")
        .flag_if_supported("-fno-exceptions")
        .flag_if_supported("-fvisibility=hidden")
        .flag_if_supported("-ffunction-sections")
        .flag_if_supported("-fdata-sections")
        .define("NDEBUG", None)
        .opt_level(2);
    if output(&llvm.config, &["--has-rtti"])? == "NO" {
        build.flag("-fno-rtti");
    }
    if std::env::var("CARGO_CFG_TARGET_OS")?.as_str() == "macos" {
        let sdk = output(Path::new("xcrun"), &["--sdk", "macosx", "--show-sdk-path"])?;
        build
            .flag("-isysroot")
            .flag(&sdk)
            .flag("-isystem")
            .flag(format!("{sdk}/usr/include/c++/v1"));
    }
    for file in ["frontend.cpp", "filesystem.cpp", "visitor.cpp"] {
        let path = format!("native/{file}");
        println!("cargo:rerun-if-changed={path}");
        build.file(path);
    }
    println!("cargo:rerun-if-changed=native/bridge.h");
    println!("cargo:rerun-if-changed=native/budget.h");
    build.compile("kodstar_clang_bridge");
    println!("cargo:rustc-link-search=native={}", llvm.lib.display());
    let mut clang: Vec<_> = std::fs::read_dir(&llvm.lib)?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.starts_with("libclang") && s.ends_with(".a"))
        })
        .collect();
    clang.sort();
    let mut archives = super::archives::Archives::open(Path::new(
        &std::env::var_os("OUT_DIR").ok_or("missing output directory")?,
    ))?;
    for archive in clang {
        link_archive(&archive, &mut identity, &mut archives)?;
    }
    for archive in output(&llvm.config, &["--link-static", "--libfiles", "all"])?.split_whitespace()
    {
        link_archive(Path::new(archive), &mut identity, &mut archives)?;
    }
    archives.finish()?;
    for library in output(&llvm.config, &["--link-static", "--system-libs"])?.split_whitespace() {
        let path = Path::new(library);
        let archive = path.file_name().and_then(|name| name.to_str());
        if library == "-lzstd"
            || path.is_absolute()
                && matches!(archive, Some("libzstd.a" | "libzstd.so" | "libzstd.dylib"))
        {
            // The workspace zstd-sys supplies the Zstd C API used by these archives.
            continue;
        }
        if let Some(name) = library.strip_prefix("-l") {
            println!("cargo:rustc-link-lib={name}");
        } else if !library.ends_with("libz3.dylib") && !library.ends_with("libz3.so") {
            return Err(format!("unsupported LLVM system link input: {library}").into());
        }
    }
    if std::env::var("CARGO_CFG_TARGET_OS")?.as_str() == "macos" {
        println!("cargo:rustc-link-lib=xcselect");
    }
    println!(
        "cargo:rustc-env=KODSTAR_CLANG_ENGINE_ID={}",
        identity.finalize().to_hex()
    );
    Ok(())
}

fn link_archive(
    path: &Path,
    identity: &mut blake3::Hasher,
    archives: &mut super::archives::Archives,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed={}", path.display());
    identity.update(archives.digest(path)?.as_bytes());
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.strip_prefix("lib"))
        .ok_or("invalid archive name")?;
    println!("cargo:rustc-link-lib=static:-bundle={name}");
    Ok(())
}
