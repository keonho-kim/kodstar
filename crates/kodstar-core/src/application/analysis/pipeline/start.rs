// Purpose: 파일 교체와 같은 선언 환경의 본문 재사용을 시작하고 변경 집합 구성.
// Pattern: State Transition.
// Usage: pipeline writer가 Begin 이벤트에 대해 같은 transaction에서 호출.
// Related: crates/kodstar-core/src/application/analysis/pipeline.rs, crates/kodstar-core/src/infrastructure/index/facts.rs.

use super::super::incremental::{self, Resolution, ReusePolicy};
use crate::domain::diagnostics::Result;
use crate::infrastructure::{
    cache::paths::path_bytes,
    index::{self, FileFacts, FileRecord, cached::Sql},
};

pub(super) fn file(
    c: &Sql<'_>,
    record: &FileRecord,
    change: &super::super::events::FileChange,
    full: bool,
    reuse: &mut ReusePolicy,
    resolution: &mut Resolution,
    budget: usize,
) -> Result<FileFacts> {
    use super::super::events::FileChange;
    let preserve = matches!(change, FileChange::Body) && *resolution != Resolution::All;
    c.execute(
        "INSERT OR IGNORE INTO seen_files VALUES(?1)",
        [path_bytes(&record.path)],
    )?;
    if !full
        && *resolution != Resolution::All
        && let FileChange::Append(candidate) = change
        && let Some(context) = index::append_reuse::start(c, record, candidate)?
    {
        *resolution = Resolution::Selected;
        return Ok(context);
    }
    let scoped = !full
        && !preserve
        && *resolution != Resolution::All
        && reuse.declarations(c, &record.language)?
        && incremental::scoped_declarations(c, record)?;
    if !preserve && !scoped {
        resolution.reset_all(c)?;
    }
    let mut granular = false;
    let id = if preserve {
        record.refresh_metadata(c)?;
        let id: i64 = c.query_row(
            "SELECT id FROM files WHERE path=?1",
            [path_bytes(&record.path)],
            |r| r.get(0),
        )?;
        granular = index::body_reuse::prepare(c, id, budget)?;
        c.execute(
            "DELETE FROM bindings WHERE file_id=?1 AND kind='clang_call'",
            [id],
        )?;
        c.execute(
            "INSERT OR IGNORE INTO body_files(id,granular) VALUES(?1,?2)",
            rusqlite::params![id, granular],
        )?;
        if !granular {
            c.execute(
                "INSERT OR IGNORE INTO affected_facts SELECT id FROM facts WHERE file_id=?1
                AND relation IN ('calls','references','dispatch_candidate')",
                [id],
            )?;
            index::statistics::delta::remember_file(c, id)?;
            index::relations::clear_bodies(c, id, false)?;
            c.execute("DELETE FROM facts WHERE file_id=?1 AND relation IN ('calls','references','dispatch_candidate')", [id])?;
        }
        id
    } else {
        let id = record.persist(c)?;
        if scoped {
            index::statistics::delta::remember_file(c, id)?;
        }
        index::clear_file(c, id)?;
        id
    };
    *resolution = (*resolution).max(if preserve || scoped {
        Resolution::Selected
    } else {
        Resolution::All
    });
    c.execute("INSERT OR IGNORE INTO changed_files VALUES(?1)", [id])?;
    let mut context = FileFacts::new(id, record, preserve);
    context.call_reuse = granular;
    Ok(context)
}
