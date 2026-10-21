// Purpose: 세션 조회 입력을 엔진의 조회·옵션 값으로 변환.
// Pattern: Boundary Parser.
// Usage: 세션 protocol decoder가 query 매개변수를 전달.
// Related: crates/kodstar-cli/src/session/protocol.rs, crates/kodstar-cli/src/invocation.rs.

use crate::invocation::QueryRequest;
use kodstar_core::{
    Depth, Direction, Error, Limit, Query, QueryOptions, Refresh, RelationKind, Result, Selector,
};
use serde::Deserialize;
use std::num::NonZeroUsize;
use std::path::PathBuf;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum QueryInput {
    Symbols {
        file: PathBuf,
    },
    List {
        file: PathBuf,
    },
    Related {
        file: PathBuf,
        name: String,
        at: Option<(usize, usize)>,
    },
    Deps {
        file: PathBuf,
        name: String,
        at: Option<(usize, usize)>,
    },
    Refs {
        file: PathBuf,
        name: String,
        at: Option<(usize, usize)>,
    },
    Explain {
        file: PathBuf,
        name: String,
        at: Option<(usize, usize)>,
    },
    Impact {
        file: PathBuf,
        name: Option<String>,
        at: Option<(usize, usize)>,
    },
    Path {
        from: String,
        to: String,
    },
    Search {
        name: String,
        #[serde(default)]
        prefix: bool,
    },
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct OptionsInput {
    refresh: Option<String>,
    direction: Option<String>,
    relations: Vec<String>,
    depth: Option<usize>,
    transitive: bool,
    members: bool,
    sites: bool,
    include_candidates: bool,
    include_external: bool,
    limit: Option<usize>,
    all: bool,
    max_nodes: Option<usize>,
}

pub(super) fn parse(query: QueryInput, raw: OptionsInput) -> Result<QueryRequest> {
    let query = match query {
        QueryInput::Symbols { file } => Query::Symbols { file },
        QueryInput::List { file } => Query::Files { file },
        QueryInput::Related { file, name, at } => Query::Related {
            selector: Selector { file, name, at },
        },
        QueryInput::Deps { file, name, at } => Query::Deps {
            selector: Selector { file, name, at },
        },
        QueryInput::Refs { file, name, at } => Query::Refs {
            selector: Selector { file, name, at },
        },
        QueryInput::Explain { file, name, at } => Query::Explain {
            selector: Selector { file, name, at },
        },
        QueryInput::Impact { file, name, at } => Query::Impact { file, name, at },
        QueryInput::Path { from, to } => Query::Path {
            from: Selector::parse(&from)?,
            to: Selector::parse(&to)?,
        },
        QueryInput::Search { name, prefix } => Query::Search { name, prefix },
    };
    if raw.depth.is_some() && raw.transitive || raw.limit.is_some() && raw.all {
        return Err(Error::argument(
            "depth/transitive or limit/all options conflict",
        ));
    }
    let options = QueryOptions {
        refresh: parse_refresh(raw.refresh.as_deref())?,
        direction: raw
            .direction
            .map(|value| value.parse::<Direction>())
            .transpose()?,
        relations: raw
            .relations
            .into_iter()
            .map(|value| value.parse::<RelationKind>())
            .collect::<Result<_>>()?,
        depth: if raw.transitive {
            Depth::Transitive
        } else {
            raw.depth
                .map(|depth| positive(depth, "depth").map(Depth::Bounded))
                .transpose()?
                .unwrap_or_default()
        },
        members: raw.members,
        sites: raw.sites,
        include_candidates: raw.include_candidates,
        include_external: raw.include_external,
        limit: if raw.all {
            Limit::Unlimited
        } else {
            raw.limit
                .map(|limit| positive(limit, "limit").map(Limit::Count))
                .transpose()?
                .unwrap_or_default()
        },
        max_nodes: match raw.max_nodes {
            None => Limit::Default,
            Some(0) => Limit::Unlimited,
            Some(value) => Limit::Count(positive(value, "max_nodes")?),
        },
    };
    Ok(QueryRequest { query, options })
}

fn positive(value: usize, name: &str) -> Result<NonZeroUsize> {
    NonZeroUsize::new(value).ok_or_else(|| Error::argument(format!("{name} must be positive")))
}

pub(super) fn parse_refresh(value: Option<&str>) -> Result<Refresh> {
    match value {
        None | Some("metadata") => Ok(Refresh::Metadata),
        Some("verify") => Ok(Refresh::Verify),
        Some("none") => Ok(Refresh::None),
        Some(_) => Err(Error::argument("refresh must be metadata, verify, or none")),
    }
}
