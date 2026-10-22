// Purpose: CLI 이력 하위 명령을 구조화된 엔진 요청으로 변환.
// Pattern: Boundary Parser.
// Usage: Invocation이 history·branch 입력을 전달.
// Related: crates/kodstar-cli/src/args/history.rs.

use super::{refresh, required, value};
use clap::ArgMatches;
use kodstar_core::{Error, HistoryRequest, Result};

pub(super) fn parse(name: &str, outer: &ArgMatches, args: &ArgMatches) -> Result<HistoryRequest> {
    Ok(match (name, outer.subcommand_name()) {
        ("history", Some("upgrade")) => HistoryRequest::Upgrade,
        ("history", None) => HistoryRequest::List {
            refresh: refresh(args),
        },
        ("history", Some("save")) => HistoryRequest::Save {
            name: required(args, "name")?,
        },
        ("history", Some("diff")) => HistoryRequest::Diff {
            from: value(args, "from"),
            to: value(args, "to"),
            refresh: refresh(args),
        },
        ("history", Some("restore")) => HistoryRequest::Restore {
            revision: required(args, "revision")?,
            path: value(args, "path"),
        },
        ("branch", None) => HistoryRequest::Branches,
        ("branch", Some("create")) => HistoryRequest::CreateBranch {
            name: required(args, "name")?,
        },
        ("branch", Some("switch")) => HistoryRequest::SwitchBranch {
            name: required(args, "name")?,
        },
        ("branch", Some("delete")) => HistoryRequest::DeleteBranch {
            name: required(args, "name")?,
        },
        _ => return Err(Error::argument("unknown history command")),
    })
}
