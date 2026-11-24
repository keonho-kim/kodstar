// Purpose: 고정 builtin header를 파일별 Zstd frame과 정렬된 메모리 색인으로 생성.
// Pattern: Resource Compiler.
// Usage: build.rs가 LLVM 개발 입력을 확인한 뒤 OUT_DIR에만 기록.
// Related: crates/kodstar-clang/build/toolchain.rs, crates/kodstar-clang/src/headers.rs.

use super::{compression::Compression, resources, toolchain::Llvm};
use std::{io::Write, path::Path};

pub(super) fn generate(llvm: &Llvm) -> Result<(), Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    for (prefix, root) in resources::roots(llvm)? {
        let mut paths = Vec::new();
        walk(&root, &mut paths)?;
        for file in paths {
            let relative = file
                .strip_prefix(&root)?
                .to_str()
                .ok_or("non-UTF-8 header resource path")?;
            files.push((format!("{prefix}/{relative}"), file));
        }
    }
    files.sort();
    let mut compression = Compression::new(&files)?;
    let mut entries = Vec::new();
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?);
    let mut bytes = std::fs::File::create(out.join("headers.pack"))?;
    let mut index = String::from("&[\n");
    let mut offset = 0usize;
    let mut identity = blake3::Hasher::new();
    let mut system_identity = blake3::Hasher::new();
    let mut notices = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (name, file) in files {
        println!("cargo:rerun-if-changed={}", file.display());
        let raw = std::fs::read(&file)?;
        if raw.len() > 8 * 1024 * 1024 {
            return Err("builtin header exceeds resource budget".into());
        }
        for block in copyright_blocks(&raw) {
            let digest = blake3::hash(block);
            if seen.insert(*digest.as_bytes()) {
                notices.extend_from_slice(
                    format!("\n===== Clang builtin header / {name} =====\n").as_bytes(),
                );
                notices.extend_from_slice(block);
                notices.push(b'\n');
            }
        }
        identity.update(name.as_bytes());
        identity.update(&blake3::hash(&raw).as_bytes()[..]);
        if name.starts_with("system/") {
            system_identity.update(&(name.len() as u64).to_le_bytes());
            system_identity.update(name.as_bytes());
            system_identity.update(blake3::hash(&raw).as_bytes());
        }
        compression.push(&raw)?;
        entries.push((name, raw.len()));
    }
    let (dictionary, frames) = compression.finish();
    std::fs::write(out.join("headers.dict"), dictionary)?;
    for ((name, raw), compressed) in entries.into_iter().zip(frames) {
        index.push_str(&format!(
            "({name:?},{offset},{},{}),\n",
            compressed.len(),
            raw
        ));
        bytes.write_all(&compressed)?;
        offset += compressed.len();
    }
    index.push_str("]\n");
    resources::verify_system(system_identity.finalize().to_hex().as_ref())?;
    std::fs::write(out.join("headers.rs"), index)?;
    std::fs::write(
        out.join("header-notices.zst"),
        zstd::bulk::compress(&notices, 9)?,
    )?;
    println!(
        "cargo:rustc-env=KODSTAR_CLANG_SYSTEM_ID={}",
        system_identity.finalize().to_hex()
    );
    println!(
        "cargo:rustc-env=KODSTAR_CLANG_RESOURCE_ID={}",
        identity.finalize().to_hex()
    );
    Ok(())
}

fn walk(root: &Path, files: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            walk(&path, files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn copyright_blocks(source: &[u8]) -> Vec<&[u8]> {
    let mut blocks = Vec::new();
    let mut at = 0;
    while let Some(start) = source[at..]
        .windows(2)
        .position(|b| b == b"/*")
        .map(|n| n + at)
    {
        let Some(end) = source[start + 2..]
            .windows(2)
            .position(|b| b == b"*/")
            .map(|n| start + 2 + n + 2)
        else {
            break;
        };
        let block = &source[start..end];
        if [
            b"SPDX".as_slice(),
            b"Copyright",
            b"copyright",
            b"Permission is hereby",
            b"Redistribution",
        ]
        .iter()
        .any(|needle| block.windows(needle.len()).any(|b| b == *needle))
        {
            blocks.push(block);
        }
        at = end;
    }
    blocks
}
