// Purpose: 설정 병합·입력 파싱·자원 예산 검증.
// Pattern: Simple Module.
// Usage: 프로젝트 선택 시 설정을 로드.
// Related: crates/kodstar-core/src/application/workspace/mod.rs.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::domain::diagnostics::{Error, ErrorKind, Result};
use crate::domain::languages::Language;

const MIB: usize = 1024 * 1024;
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub exclude: Vec<String>,
    pub language_overrides: BTreeMap<String, Language>,
    pub jobs: usize,
    pub memory_budget: usize,
    pub max_file_bytes: usize,
    pub fact_batch_bytes: usize,
    pub queue_bytes: usize,
    pub db_cache_bytes: usize,
    pub limit: usize,
    pub max_nodes: usize,
    pub cache_ttl_days: u64,
    pub cache_budget_bytes: u64,
    pub auto_gc_interval_seconds: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            exclude: Vec::new(),
            language_overrides: BTreeMap::new(),
            jobs: 2,
            memory_budget: 256 * MIB,
            max_file_bytes: 2 * MIB,
            fact_batch_bytes: MIB,
            queue_bytes: 8 * MIB,
            db_cache_bytes: 8 * MIB,
            limit: 200,
            max_nodes: 100_000,
            cache_ttl_days: 30,
            cache_budget_bytes: 2 * 1024 * MIB as u64,
            auto_gc_interval_seconds: 24 * 60 * 60,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SettingsOverrides {
    pub jobs: Option<usize>,
    pub memory_budget: Option<usize>,
    pub max_file_bytes: Option<usize>,
}

impl Settings {
    pub(crate) fn input_per_worker(&self) -> Result<usize> {
        self.max_file_bytes
            .checked_mul(3)
            .and_then(|bytes| {
                self.fact_batch_bytes
                    .checked_mul(6)
                    .and_then(|staging| bytes.checked_add(staging))
            })
            .and_then(|bytes| bytes.checked_add(512 * 1024))
            .ok_or_else(|| Error::argument("input and fact staging budget overflow"))
    }
    pub(crate) fn load(
        home: &Path,
        explicit: Option<&Path>,
        overrides: &SettingsOverrides,
    ) -> Result<Self> {
        let mut builder = config::Config::builder();
        let global = home.join("config.toml");
        for (path, required) in [(Some(global.as_path()), false), (explicit, true)] {
            let Some(path) = path else { continue };
            let file = match File::open(path) {
                Ok(file) => file,
                Err(error) if !required && error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(Error::io("open configuration", path, error)),
            };
            let mut text = String::new();
            file.take(MAX_CONFIG_BYTES + 1)
                .read_to_string(&mut text)
                .map_err(|error| Error::io("read configuration", path, error))?;
            if text.len() as u64 > MAX_CONFIG_BYTES {
                return Err(Error::argument(format!(
                    "configuration exceeds {MAX_CONFIG_BYTES} bytes: {}",
                    path.display()
                )));
            }
            builder = builder.add_source(config::File::from_str(&text, config::FileFormat::Toml));
        }
        let mut settings: Self = builder
            .build()
            .map_err(|error| Error::private_input(ErrorKind::Argument, "invalid TOML configuration; check syntax (input excerpt omitted)", error))?
            .try_deserialize()
            .map_err(|error| Error::private_input(ErrorKind::Argument, "invalid configuration fields; check supported keys and value types (input excerpt omitted)", error))?;
        if let Some(jobs) = overrides.jobs {
            settings.jobs = jobs;
        }
        if let Some(bytes) = overrides.memory_budget {
            settings.memory_budget = bytes;
        }
        if let Some(bytes) = overrides.max_file_bytes {
            settings.max_file_bytes = bytes;
        }
        settings.validate()?;
        Ok(settings)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if !(1..=64).contains(&self.jobs) {
            return Err(Error::argument("jobs must be between 1 and 64"));
        }
        if self.max_file_bytes == 0
            || self.fact_batch_bytes == 0
            || self.queue_bytes == 0
            || self.db_cache_bytes < 1024
            || self.limit == 0
            || self.max_nodes == 0
        {
            return Err(Error::argument(
                "input, batch, queue, cache, and default query limits must be positive",
            ));
        }
        let controlled = self.controlled_bytes()?;
        if controlled > self.memory_budget {
            return Err(Error::argument(
                "memory_budget must cover worker inputs, queued facts, three analysis DB caches, history/snapshot caches and storage buffers; AST/RSS are additional runtime costs",
            ));
        }
        if self.max_file_bytes > i32::MAX as usize || self.fact_batch_bytes > self.queue_bytes {
            return Err(Error::argument(
                "max_file_bytes must fit parser offsets and fact_batch_bytes must not exceed queue_bytes",
            ));
        }
        for pattern in self.exclude.iter().chain(self.language_overrides.keys()) {
            globset::Glob::new(pattern).map_err(|error| {
                Error::caused_by(format!("invalid path pattern {pattern:?}"), error)
            })?;
        }
        Ok(())
    }

    fn controlled_bytes(&self) -> Result<usize> {
        self.input_per_worker()?
            .checked_mul(self.jobs)
            .and_then(|bytes| bytes.checked_add(self.queue_bytes))
            .and_then(|bytes| bytes.checked_add(self.fact_batch_bytes))
            .and_then(|bytes| {
                self.db_cache_bytes
                    .checked_mul(3)
                    .and_then(|cache| bytes.checked_add(cache))
            })
            .and_then(|bytes| {
                bytes.checked_add(
                    crate::infrastructure::history::CACHE_BYTES
                        + crate::infrastructure::history::SNAPSHOT_CACHE_BYTES
                        + 512 * 1024,
                )
            })
            .ok_or_else(|| Error::argument("resource settings overflow the host address space"))
    }

    pub(crate) fn native_phase_bytes(&self) -> Result<usize> {
        // Native workspace excludes inputs, queued work and owned worker stacks; parsing releases it before resolution.
        let stacks = self
            .jobs
            .saturating_sub(1)
            .checked_mul(crate::application::analysis::resources::GENERAL_STACK_BYTES)
            .and_then(|bytes| {
                bytes.checked_add(crate::application::analysis::resources::WORKER_STACK_BYTES)
            })
            .ok_or_else(|| Error::argument("analysis stack reservation overflow"))?;
        self.memory_budget
            .checked_sub(self.controlled_bytes()?.saturating_add(stacks))
            .ok_or_else(|| {
                Error::argument("analysis memory budget must also cover the owned worker stacks")
            })
    }

    pub(crate) fn fingerprint(&self) -> Result<String> {
        let data = serde_json::to_vec(&(
            &self.exclude,
            &self.language_overrides,
            self.max_file_bytes,
            self.fact_batch_bytes,
        ))
        .map_err(|error| Error::caused_by("encode analysis configuration fingerprint", error))?;
        Ok(blake3::hash(&data).to_hex().to_string())
    }
}
