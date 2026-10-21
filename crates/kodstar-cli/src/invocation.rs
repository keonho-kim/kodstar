// Purpose: clap 입력을 명시적인 엔진 요청과 출력 형식으로 변환.
// Pattern: Boundary Parser.
// Usage: main이 파싱한 인자를 실행 경계에 전달.
// Related: crates/kodstar-cli/src/args.rs, crates/kodstar-cli/src/run.rs.

use clap::ArgMatches;
use kodstar_core::{
    BuildOptions, CacheRequest, Depth, Direction, Error, Limit, ProjectOptions, Query,
    QueryOptions, Refresh, RelationKind, Result, Selector, SettingsOverrides,
};
use std::num::NonZeroUsize;
use std::path::PathBuf;

#[derive(Clone, Copy)]
pub(crate) enum Format {
    Text,
    Json,
    Jsonl,
    Tsv,
}

pub(crate) struct QueryRequest {
    pub query: Query,
    pub options: QueryOptions,
}

pub(crate) enum Action {
    Build(BuildOptions),
    Query(Box<QueryRequest>),
    Status {
        doctor: bool,
        refresh: Refresh,
    },
    Languages {
        refresh: Refresh,
    },
    Watch,
    Serve,
    Cache {
        request: CacheRequest,
        refresh: Refresh,
    },
}

pub(crate) struct Invocation {
    pub project: Option<ProjectOptions>,
    pub action: Action,
    pub format: Format,
}

impl Invocation {
    pub(crate) fn parse(matches: &ArgMatches) -> Result<Self> {
        let (name, outer) = matches
            .subcommand()
            .ok_or_else(|| Error::argument("a command is required"))?;
        let args = if name == "cache" {
            outer
                .subcommand()
                .map(|(_, args)| args)
                .ok_or_else(|| Error::argument("a cache command is required"))?
        } else {
            outer
        };
        let format = match value::<String>(args, "format").as_deref() {
            Some("json") => Format::Json,
            Some("jsonl") => Format::Jsonl,
            Some("tsv") => Format::Tsv,
            _ => Format::Text,
        };
        let project = if name == "serve" {
            None
        } else {
            let cwd = std::env::current_dir()
                .map_err(|error| Error::caused_by("read current directory", error))?;
            Some(ProjectOptions {
                cwd,
                root: value(args, "root"),
                config: value(args, "config"),
                overrides: SettingsOverrides {
                    jobs: value(args, "jobs"),
                    memory_budget: value(args, "memory-budget"),
                    max_file_bytes: value(args, "max-file-bytes"),
                },
            })
        };
        let action = match name {
            "build" => Action::Build(BuildOptions {
                path: value(args, "path"),
                full: flag(args, "full"),
                verify: flag(args, "verify"),
            }),
            "status" | "doctor" => Action::Status {
                doctor: name == "doctor",
                refresh: refresh(args),
            },
            "languages" => Action::Languages {
                refresh: refresh(args),
            },
            "watch" => Action::Watch,
            "serve" => Action::Serve,
            "cache" => Action::Cache {
                request: match outer.subcommand_name() {
                    Some("list") => CacheRequest::List,
                    Some("clean") => CacheRequest::Clean {
                        all: flag(args, "all"),
                    },
                    Some("gc") => CacheRequest::Gc {
                        dry_run: flag(args, "dry-run"),
                    },
                    _ => return Err(Error::argument("unknown cache command")),
                },
                refresh: if outer.subcommand_name() == Some("list") {
                    refresh(args)
                } else {
                    Refresh::None
                },
            },
            _ => Action::Query(Box::new(QueryRequest {
                query: query(name, args)?,
                options: query_options(args),
            })),
        };
        Ok(Self {
            project,
            action,
            format,
        })
    }
}

fn query(name: &str, args: &ArgMatches) -> Result<Query> {
    if name == "path" {
        return Ok(Query::Path {
            from: Selector::parse(&required::<String>(args, "from")?)?,
            to: Selector::parse(&required::<String>(args, "to")?)?,
        });
    }
    if name == "search" {
        return Ok(Query::Search {
            name: required(args, "name")?,
            prefix: flag(args, "prefix"),
        });
    }
    let file: PathBuf = required(args, "file")?;
    match name {
        "symbols" => return Ok(Query::Symbols { file }),
        "list" => return Ok(Query::Files { file }),
        "impact" => {
            return Ok(Query::Impact {
                file,
                name: value(args, "name"),
                at: value(args, "at"),
            });
        }
        _ => {}
    }
    let selector = Selector {
        file,
        name: required(args, "name")?,
        at: value(args, "at"),
    };
    match name {
        "related" => Ok(Query::Related { selector }),
        "deps" => Ok(Query::Deps { selector }),
        "refs" => Ok(Query::Refs { selector }),
        "explain" => Ok(Query::Explain { selector }),
        _ => Err(Error::argument("unknown query command")),
    }
}

fn query_options(args: &ArgMatches) -> QueryOptions {
    QueryOptions {
        refresh: refresh(args),
        direction: value::<Direction>(args, "direction"),
        relations: args
            .try_get_many::<RelationKind>("relation")
            .ok()
            .flatten()
            .map(|items| items.copied().collect())
            .unwrap_or_default(),
        depth: if flag(args, "transitive") {
            Depth::Transitive
        } else {
            value::<NonZeroUsize>(args, "depth").map_or(Depth::Default, Depth::Bounded)
        },
        members: flag(args, "members"),
        sites: flag(args, "sites"),
        include_candidates: flag(args, "include-candidates"),
        include_external: flag(args, "include-external"),
        limit: if flag(args, "all") {
            Limit::Unlimited
        } else {
            value::<usize>(args, "limit")
                .and_then(NonZeroUsize::new)
                .map_or(Limit::Default, Limit::Count)
        },
        max_nodes: match value::<usize>(args, "max-nodes") {
            Some(0) => Limit::Unlimited,
            Some(value) => NonZeroUsize::new(value).map_or(Limit::Default, Limit::Count),
            None => Limit::Default,
        },
    }
}

fn value<T: Clone + Send + Sync + 'static>(args: &ArgMatches, name: &str) -> Option<T> {
    args.try_get_one::<T>(name).ok().flatten().cloned()
}

fn flag(args: &ArgMatches, name: &str) -> bool {
    value(args, name).unwrap_or(false)
}

fn required<T: Clone + Send + Sync + 'static>(args: &ArgMatches, name: &str) -> Result<T> {
    value(args, name).ok_or_else(|| Error::argument(format!("missing {name}")))
}

fn refresh(args: &ArgMatches) -> Refresh {
    if flag(args, "no-refresh") {
        Refresh::None
    } else if flag(args, "verify") {
        Refresh::Verify
    } else {
        Refresh::Metadata
    }
}

impl Action {
    pub(crate) fn command(&self) -> Result<kodstar_core::Command<'_>> {
        use kodstar_core::Command;
        Ok(match self {
            Self::Build(options) => Command::Build(options),
            Self::Query(request) => Command::Query {
                query: &request.query,
                options: &request.options,
            },
            Self::Status { doctor, refresh } => Command::Status {
                doctor: *doctor,
                refresh: *refresh,
            },
            Self::Languages { refresh } => Command::Languages { refresh: *refresh },
            Self::Cache { request, refresh } => Command::Cache {
                request: *request,
                refresh: *refresh,
            },
            Self::Watch => Command::Watch,
            Self::Serve => {
                return Err(Error::argument(
                    "session transport is not an engine command",
                ));
            }
        })
    }
}
