// Purpose: 조회 요청의 이름·경로·위치·의미 있는 옵션 조합 검증.
// Pattern: Boundary Validation.
// Usage: Engine 조회 진입 시 QueryOptions가 호출.
// Related: crates/kodstar-core/src/domain/query.rs, crates/kodstar-core/src/application/engine.rs.

use super::{Depth, Direction, Error, Limit, Query, QueryOptions, Result, Selector};
use std::path::Path;

pub(super) fn validate(options: &QueryOptions, query: &Query) -> Result<()> {
    if options
        .revision
        .as_ref()
        .is_some_and(|s| s.trim().is_empty() || s.contains('\0'))
    {
        return Err(Error::argument(
            "checkpoint selector must be nonempty and contain no NUL",
        ));
    }
    match query {
        Query::Changes { since } => {
            name(since)?;
            if options.revision.is_some() {
                return Err(Error::argument("--since cannot be combined with --rev"));
            }
        }
        Query::Symbols { file } | Query::Files { file } => path(file)?,
        Query::Search { name: value, .. } => name(value)?,
        Query::RankedSearch { text } => {
            if text.len() > 256 || text.contains('\0') || !text.chars().any(char::is_alphanumeric) {
                return Err(Error::argument(
                    "ranked search requires letters or digits in at most 256 bytes",
                ));
            }
        }
        Query::Overview => {}
        Query::Related { selector }
        | Query::Deps { selector }
        | Query::Refs { selector }
        | Query::Explain { selector } => object(selector)?,
        Query::Path { from, to } => {
            object(from)?;
            object(to)?;
        }
        Query::Impact {
            file,
            name: value,
            at,
        } => {
            path(file)?;
            if let Some(value) = value {
                name(value)?;
            } else if at.is_some() {
                return Err(Error::argument(
                    "an object name is required with a source position",
                ));
            }
            position(*at)?;
        }
    }
    if options.direction == Some(Direction::Undirected) && !matches!(query, Query::Path { .. }) {
        return Err(Error::argument(
            "undirected traversal is only available for path",
        ));
    }
    if matches!(query, Query::Deps { .. })
        && options
            .direction
            .is_some_and(|value| value != Direction::Out)
    {
        return Err(Error::argument(
            "deps cannot traverse incoming relationships",
        ));
    }
    if matches!(
        query,
        Query::Refs { .. } | Query::Impact { .. } | Query::Changes { .. }
    ) && options
        .direction
        .is_some_and(|value| value != Direction::In)
    {
        return Err(Error::argument(
            "refs and impact cannot traverse outgoing relationships",
        ));
    }
    let definitions = matches!(
        query,
        Query::Symbols { .. } | Query::Search { .. } | Query::Overview
    );
    let traversal = matches!(
        query,
        Query::Related { .. }
            | Query::Deps { .. }
            | Query::Refs { .. }
            | Query::Impact { .. }
            | Query::Changes { .. }
            | Query::Path { .. }
    );
    if definitions
        && (options.direction.is_some()
            || !options.relations.is_empty()
            || options.include_candidates
            || options.include_external)
    {
        return Err(Error::argument(
            "relationship filters do not apply to definition queries",
        ));
    }
    if !traversal && !matches!(options.depth, Depth::Default) {
        return Err(Error::argument("depth requires a traversal query"));
    }
    let supports_visits =
        traversal || matches!(query, Query::Overview | Query::RankedSearch { .. });
    if !supports_visits && !matches!(options.max_nodes, Limit::Default) {
        return Err(Error::argument(
            "this query does not accept a visited-item limit",
        ));
    }
    if options.sites && (definitions || matches!(query, Query::Files { .. })) {
        return Err(Error::argument(
            "individual sites require an object relationship query",
        ));
    }
    if options.members
        && !matches!(
            query,
            Query::Related { .. } | Query::Deps { .. } | Query::Refs { .. } | Query::Explain { .. }
        )
    {
        return Err(Error::argument(
            "member selection requires related, deps, refs, or explain",
        ));
    }
    Ok(())
}

fn object(selector: &Selector) -> Result<()> {
    path(&selector.file)?;
    name(&selector.name)?;
    position(selector.at)
}

fn name(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.contains('\0') {
        return Err(Error::argument(
            "object name must be nonempty and contain no NUL",
        ));
    }
    Ok(())
}

fn path(value: &Path) -> Result<()> {
    if value.as_os_str().is_empty() || value.to_string_lossy().contains('\0') {
        return Err(Error::argument(
            "file path must be nonempty and contain no NUL",
        ));
    }
    Ok(())
}

fn position(value: Option<(usize, usize)>) -> Result<()> {
    if value.is_some_and(|(line, column)| line == 0 || column == 0) {
        return Err(Error::argument(
            "source line and UTF-8 byte column must start at 1",
        ));
    }
    Ok(())
}
