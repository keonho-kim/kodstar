// Purpose: 제한된 세션 JSON의 버전·ID·메서드와 요청 매개변수 파싱.
// Pattern: Boundary Parser.
// Usage: 세션 dispatcher가 한 프레임을 해석하여 작업이나 제어 요청을 얻음.
// Related: crates/kodstar-cli/src/session/mod.rs, crates/kodstar-cli/src/session/query.rs.

use super::query::{self, OptionsInput, QueryInput};
use crate::invocation::Action;
use kodstar_core::{
    BuildOptions, CacheRequest, Error, ErrorKind, ProjectOptions, Result, SettingsOverrides,
};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub(super) enum Id {
    Text(String),
    Number(i64),
}

pub(super) struct Work {
    pub project: Option<ProjectOptions>,
    pub action: Action,
}
pub(super) enum Operation {
    Run(Box<Work>),
    Cancel(Id),
    Shutdown,
}
pub(super) struct Request {
    pub id: Id,
    pub operation: Operation,
}
pub(super) struct Rejected {
    pub id: Option<Id>,
    pub error: Error,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRequest {
    protocol_version: u32,
    id: Id,
    method: String,
    #[serde(default)]
    params: Option<Box<RawValue>>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ProjectInput {
    root: Option<PathBuf>,
    config: Option<PathBuf>,
    jobs: Option<usize>,
    memory_budget: Option<usize>,
    max_file_bytes: Option<usize>,
}
impl ProjectInput {
    fn options(self, cwd: &Path) -> ProjectOptions {
        ProjectOptions {
            cwd: cwd.to_path_buf(),
            root: self.root,
            config: self.config,
            overrides: SettingsOverrides {
                jobs: self.jobs,
                memory_budget: self.memory_budget,
                max_file_bytes: self.max_file_bytes,
            },
        }
    }
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct BuildInput {
    project: ProjectInput,
    path: Option<PathBuf>,
    full: bool,
    verify: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryParameters {
    #[serde(default)]
    project: ProjectInput,
    query: QueryInput,
    #[serde(default)]
    options: OptionsInput,
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ProjectParameters {
    project: ProjectInput,
    options: SyncOptions,
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct SyncOptions {
    refresh: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CacheParameters {
    #[serde(default)]
    project: ProjectInput,
    action: String,
    #[serde(default)]
    all: bool,
    #[serde(default)]
    dry_run: bool,
    #[serde(default)]
    options: SyncOptions,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cancel {
    request_id: Id,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Deserialize)]
struct RequestId {
    id: Id,
}

pub(super) fn decode(frame: &[u8], cwd: &Path) -> std::result::Result<Request, Rejected> {
    // Recovery reads only the bounded ID. The full typed parse still rejects duplicate and unknown fields.
    let id = serde_json::from_slice::<RequestId>(frame)
        .ok()
        .map(|header| header.id)
        .filter(|id| match id {
            Id::Text(text) => !text.is_empty() && text.len() <= 256,
            Id::Number(_) => true,
        });
    let wire: WireRequest = serde_json::from_slice(frame).map_err(|error| Rejected {
        id: id.clone(),
        error: argument("invalid request fields", error),
    })?;
    if wire.protocol_version != 1 || id.is_none() {
        return Err(Rejected {
            id,
            error: Error::argument(
                "expected protocol_version 1 and an integer or 1..256-byte string id",
            ),
        });
    }
    let operation = operation(&wire.method, wire.params, cwd).map_err(|error| Rejected {
        id: id.clone(),
        error,
    })?;
    Ok(Request {
        id: wire.id,
        operation,
    })
}

fn parameters<T: serde::de::DeserializeOwned>(value: Option<Box<RawValue>>) -> Result<T> {
    serde_json::from_str(value.as_deref().map_or("{}", RawValue::get))
        .map_err(|error| argument("invalid method parameters", error))
}

fn operation(method: &str, params: Option<Box<RawValue>>, cwd: &Path) -> Result<Operation> {
    let (project, action) = match method {
        "cancel" => return Ok(Operation::Cancel(parameters::<Cancel>(params)?.request_id)),
        "shutdown" => {
            parameters::<Empty>(params)?;
            return Ok(Operation::Shutdown);
        }
        "languages" => {
            let params: ProjectParameters = parameters(params)?;
            let refresh = query::parse_refresh(params.options.refresh.as_deref())?;
            (
                Some(params.project.options(cwd)),
                Action::Languages { refresh },
            )
        }
        "build" => {
            let params: BuildInput = parameters(params)?;
            (
                Some(params.project.options(cwd)),
                Action::Build(BuildOptions {
                    path: params.path,
                    full: params.full,
                    verify: params.verify,
                }),
            )
        }
        "query" => {
            let params: QueryParameters = parameters(params)?;
            (
                Some(params.project.options(cwd)),
                Action::Query(Box::new(query::parse(params.query, params.options)?)),
            )
        }
        "status" | "doctor" => {
            let params: ProjectParameters = parameters(params)?;
            (
                Some(params.project.options(cwd)),
                Action::Status {
                    doctor: method == "doctor",
                    refresh: query::parse_refresh(params.options.refresh.as_deref())?,
                },
            )
        }
        "cache" => {
            let params: CacheParameters = parameters(params)?;
            let request = match params.action.as_str() {
                "list" if !params.all && !params.dry_run => CacheRequest::List,
                "clean" if !params.dry_run => CacheRequest::Clean { all: params.all },
                "gc" if !params.all => CacheRequest::Gc {
                    dry_run: params.dry_run,
                },
                _ => return Err(Error::argument("invalid cache action or options")),
            };
            let refresh = if matches!(request, CacheRequest::List) {
                query::parse_refresh(params.options.refresh.as_deref())?
            } else {
                query::parse_refresh(Some(params.options.refresh.as_deref().unwrap_or("none")))?
            };
            (
                Some(params.project.options(cwd)),
                Action::Cache { request, refresh },
            )
        }
        _ => return Err(Error::argument("unknown session method")),
    };
    Ok(Operation::Run(Box::new(Work { project, action })))
}

fn argument(context: &str, error: serde_json::Error) -> Error {
    let mut error = Error::caused_by(context, error);
    error.kind = ErrorKind::Argument;
    error
}
