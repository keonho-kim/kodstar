// Purpose: 체크포인트 native 입력·한 파일의 의미 결과·재추출 재사용 수명 소유.
// Pattern: Native Adapter.
// Usage: parser worker가 설정하고 C/C++ 파일 추출에서 호출.
// Related: crates/kodstar-core/src/infrastructure/clang/inputs.rs, crates/kodstar-core/src/infrastructure/clang/projection.rs.

mod configuration;
mod generated;
mod inputs;
mod projection;
mod structural;
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, ErrorKind, Result},
    languages::Language,
    native::Report,
};
use crate::infrastructure::history::Snapshot;
use kodstar_clang::{Analysis, Status};
pub(crate) use projection::Projection;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub(crate) struct Output {
    pub analysis: Analysis,
    pub report: Arc<Report>,
}
#[derive(Clone)]
pub(crate) struct Configuration {
    pub snapshot: Snapshot,
    pub root: PathBuf,
    pub max_file: usize,
    pub budget: usize,
    pub excludes: Vec<String>,
    pub headers: Arc<kodstar_clang::HeaderDecoder>,
}
pub(crate) struct Session {
    config: Configuration,
    input: Option<inputs::ProjectInputs>,
    replay: Option<Arc<Output>>,
    commands: Option<configuration::Database>,
}

impl Session {
    pub fn report(&self) -> Option<Arc<Report>> {
        self.replay.as_ref().map(|r| Arc::clone(&r.report))
    }
    pub fn new(config: Configuration) -> Self {
        Self {
            config,
            input: None,
            replay: None,
            commands: None,
        }
    }
    pub fn clear(&mut self) {
        self.replay = None;
    }
    pub fn analyze(
        &mut self,
        path: &Path,
        source: &[u8],
        language: Language,
        cancel: &Cancellation,
    ) -> Result<Arc<Output>> {
        if let Some(result) = &self.replay {
            return Ok(Arc::clone(result));
        }
        let input = match &mut self.input {
            Some(input) => input,
            slot @ None => slot.insert(inputs::ProjectInputs::new(
                &self.config.snapshot,
                self.config.max_file,
                (self.config.budget / 16).min(4 * 1024 * 1024),
                &self.config.excludes,
                cancel,
            )?),
        };
        input.begin(cancel);
        let context =
            match configuration::prepare(input, &self.config.root, path, &mut self.commands) {
                Ok(context) => context,
                Err(error) if error.kind == ErrorKind::Limit => configuration::unavailable(),
                Err(error) => return Err(error),
            };
        let mut environment = blake3::Hasher::new();
        environment.update(kodstar_clang::VERSION.as_bytes());
        environment.update(kodstar_clang::RESOURCE_ID.as_bytes());
        environment.update(kodstar_clang::ENGINE_ID.as_bytes());
        environment.update(language.as_str().as_bytes());
        environment.update(&(self.config.budget as u64).to_le_bytes());
        let mut analysis =
            if let (Some(mut arguments), Some(path)) = (context.arguments, path.to_str()) {
                let target = format!("/__kodstar_project/{path}");
                let (main_path, main_source) = context
                    .owner
                    .as_ref()
                    .map_or((target.as_str(), source), |(path, bytes)| {
                        (path.as_str(), bytes.as_slice())
                    });
                environment.update(main_path.as_bytes());
                arguments.push(format!("--target={}", kodstar_clang::TARGET));
                if let Some(root) = self.config.root.to_str() {
                    arguments.push(format!("-ffile-prefix-map=/__kodstar_project={root}"));
                }
                for argument in &arguments {
                    environment.update(&(argument.len() as u64).to_le_bytes());
                    environment.update(argument.as_bytes());
                }
                kodstar_clang::analyze(
                    kodstar_clang::Input {
                        path: main_path,
                        source: main_source,
                        target: Some((&target, source.len())),
                        cpp: language == Language::Cpp,
                        arguments: &arguments,
                        limits: kodstar_clang::Limits::for_workspace(self.config.budget),
                    },
                    input,
                    &self.config.headers,
                )
            } else {
                Analysis {
                    status: if input.limit_issue.is_some() {
                        Status::Limited
                    } else {
                        Status::Incomplete
                    },
                    facts: kodstar_clang::Facts::default(),
                    issue: Some(kodstar_clang::Issue {
                        kind: kodstar_clang::IssueKind::CompileCommand,
                        subject: "ambiguous or unsupported compile command".into(),
                        used: 0,
                        limit: 0,
                    }),
                }
            };
        cancel.check()?;
        if let Some(error) = input.failure.take()
            && !matches!(error.kind, ErrorKind::Limit)
        {
            return Err(Error::caused_by("read native checkpoint input", error));
        }
        if let Some(issue) = input.limit_issue.take() {
            analysis.issue = Some(issue)
        }
        let status = match analysis.status {
            Status::Complete => "complete",
            Status::Limited => "limited",
            _ => "incomplete",
        };
        let detail = analysis.issue.as_ref().map(|issue| {
            serde_json::json!({
                "kind":format!("{:?}",issue.kind),
                "subject":issue.subject,
                "used":issue.used,
                "limit":issue.limit,
            })
            .to_string()
        });
        let result = Arc::new(Output {
            report: Arc::new(Report {
                environment: environment.finalize().to_hex().to_string(),
                status,
                detail,
                inputs: std::mem::take(&mut input.dependencies),
            }),
            analysis,
        });
        self.replay = Some(Arc::clone(&result));
        Ok(result)
    }
}

pub(crate) use inputs::observe;

pub(crate) fn fingerprint(settings: &str, budget: usize) -> String {
    let mut hash = blake3::Hasher::new();
    for input in [
        settings,
        kodstar_clang::VERSION,
        kodstar_clang::RESOURCE_ID,
        kodstar_clang::TARGET,
        kodstar_clang::ENGINE_ID,
    ] {
        hash.update(&(input.len() as u64).to_le_bytes());
        hash.update(input.as_bytes());
    }
    hash.update(&(budget as u64).to_le_bytes());
    hash.finalize().to_hex().to_string()
}
