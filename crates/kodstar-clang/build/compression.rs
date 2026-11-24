// Purpose: 고정 header 표본의 dictionary 학습과 전체 자산의 순압축 이득 선택.
// Pattern: Resource Compiler.
// Usage: headers가 제한된 표본으로 생성하고 원문별 frame을 전달.
// Related: crates/kodstar-clang/build/headers.rs, crates/kodstar-clang/src/headers.rs.

use std::{io::Read, path::PathBuf};

type Failure = Box<dyn std::error::Error>;

pub(super) struct Compression {
    dictionary: Vec<u8>,
    encoder: zstd::bulk::Compressor<'static>,
    decoder: zstd::bulk::Decompressor<'static>,
    frames: Vec<(Vec<u8>, Vec<u8>)>,
    independent_bytes: usize,
    dictionary_bytes: usize,
}

impl Compression {
    pub fn new(files: &[(String, PathBuf)]) -> Result<Self, Failure> {
        let sample_bytes = (16 * 1024 * 1024 / files.len().max(1)).min(4096);
        let mut samples = Vec::new();
        for (_, path) in files {
            let mut sample = Vec::new();
            std::fs::File::open(path)?
                .take(sample_bytes as u64)
                .read_to_end(&mut sample)?;
            if !sample.is_empty() {
                samples.push(sample);
            }
        }
        let dictionary = zstd::dict::from_samples(&samples, 32768)?;
        Ok(Self {
            encoder: zstd::bulk::Compressor::with_dictionary(9, &dictionary)?,
            decoder: zstd::bulk::Decompressor::with_dictionary(&dictionary)?,
            dictionary,
            frames: Vec::new(),
            independent_bytes: 0,
            dictionary_bytes: 0,
        })
    }

    pub fn push(&mut self, source: &[u8]) -> Result<(), Failure> {
        let independent = zstd::bulk::compress(source, 9)?;
        let trained = self.encoder.compress(source)?;
        if self.decoder.decompress(&trained, source.len())? != source
            || self.decoder.decompress(&independent, source.len())? != source
        {
            return Err("trained header frame changed the original source".into());
        }
        self.independent_bytes += independent.len();
        self.dictionary_bytes += independent.len().min(trained.len());
        self.frames.push((independent, trained));
        Ok(())
    }

    pub fn finish(self) -> (Vec<u8>, Vec<Vec<u8>>) {
        let trained = self.dictionary_bytes + self.dictionary.len() < self.independent_bytes;
        let frames = self
            .frames
            .into_iter()
            .map(|(independent, dictionary)| {
                if trained && dictionary.len() < independent.len() {
                    dictionary
                } else {
                    independent
                }
            })
            .collect();
        (if trained { self.dictionary } else { Vec::new() }, frames)
    }
}
