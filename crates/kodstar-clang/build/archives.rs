// Purpose: 같은 native build 출력 경로에서 검증한 정적 archive의 내용 digest 재사용.
// Pattern: Build Cache.
// Usage: toolchain 조정자가 linker 순서대로 digest를 요청하고 완료 후 저장.
// Related: crates/kodstar-clang/build/toolchain.rs, crates/kodstar-clang/build.rs.

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

const MAX_BYTES: u64 = 128 * 1024;

pub(super) struct Archives {
    path: PathBuf,
    previous: BTreeMap<[u8; 32], blake3::Hash>,
    entries: BTreeMap<[u8; 32], blake3::Hash>,
}

impl Archives {
    pub(super) fn open(directory: &Path) -> std::io::Result<Self> {
        let path = directory.join("llvm-archive-digests-v1");
        let mut bytes = Vec::new();
        match File::open(&path) {
            Ok(file) => {
                file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let mut previous = BTreeMap::new();
        if bytes.len() as u64 <= MAX_BYTES
            && let Ok(text) = std::str::from_utf8(&bytes)
        {
            for line in text.lines() {
                if let Some((key, digest)) = line.split_once(' ')
                    && let (Ok(key), Ok(digest)) =
                        (blake3::Hash::from_hex(key), blake3::Hash::from_hex(digest))
                {
                    previous.insert(*key.as_bytes(), digest);
                }
            }
        }
        Ok(Self {
            path,
            previous,
            entries: BTreeMap::new(),
        })
    }

    pub(super) fn digest(&mut self, path: &Path) -> std::io::Result<blake3::Hash> {
        let before = stamp(path, &fs::metadata(path)?)?;
        let digest = if let Some(value) = self.previous.get(&before) {
            *value
        } else {
            let mut file = File::open(path)?;
            if stamp(path, &file.metadata()?)? != before {
                return Err(changed());
            }
            let mut digest = blake3::Hasher::new();
            std::io::copy(&mut file, &mut digest)?;
            if stamp(path, &file.metadata()?)? != before
                || stamp(path, &fs::metadata(path)?)? != before
            {
                return Err(changed());
            }
            digest.finalize()
        };
        self.entries.insert(before, digest);
        Ok(digest)
    }

    pub(super) fn finish(self) -> std::io::Result<()> {
        if self.previous == self.entries {
            return Ok(());
        }
        let text = self
            .entries
            .iter()
            .fold(String::new(), |mut output, (key, digest)| {
                output.push_str(&blake3::Hash::from_bytes(*key).to_hex());
                output.push(' ');
                output.push_str(&digest.to_hex());
                output.push('\n');
                output
            });
        if text.len() as u64 > MAX_BYTES {
            return Err(std::io::Error::other(
                "LLVM archive digest cache budget exceeded",
            ));
        }
        let temporary = self.path.with_extension("tmp");
        fs::write(&temporary, text)?;
        if let Err(error) = fs::rename(&temporary, &self.path) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        Ok(())
    }
}

fn stamp(path: &Path, metadata: &fs::Metadata) -> std::io::Result<[u8; 32]> {
    if !metadata.is_file() {
        return Err(std::io::Error::other(format!(
            "static archive is not a file: {}",
            path.display()
        )));
    }
    let mut hash = blake3::Hasher::new();
    let bytes = path.as_os_str().as_encoded_bytes();
    hash.update(&(bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
    for value in [
        metadata.dev(),
        metadata.ino(),
        metadata.size(),
        metadata.mtime() as u64,
        metadata.mtime_nsec() as u64,
        metadata.ctime() as u64,
        metadata.ctime_nsec() as u64,
    ] {
        hash.update(&value.to_le_bytes());
    }
    Ok(*hash.finalize().as_bytes())
}

fn changed() -> std::io::Error {
    std::io::Error::other("LLVM archive changed while reading its digest")
}
