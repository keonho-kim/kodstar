// Purpose: 공유 체크포인트 reader로 참조 설정의 사실을 제한적으로 추출.
// Pattern: Input Adapter.
// Usage: configurations가 동일 snapshot·statement를 빌려 파일별 호출.
// Related: crates/kodstar-core/src/application/analysis/configurations.rs, crates/kodstar-core/src/infrastructure/history/inputs/mod.rs.

use super::module_identity::{module_identity, package_path};
use crate::application::workspace::SelectedProject;
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, ErrorKind, Result},
    languages::Language,
};
use crate::infrastructure::index::cached::Sql as Connection;
use crate::infrastructure::{formats, index, parse::ParseSession};
use std::path::Path;

pub(super) struct File<'a> {
    pub id: i64,
    pub path: &'a Path,
}

pub(super) fn parse(
    selected: &SelectedProject,
    inputs: &mut crate::infrastructure::history::inputs::Reader<'_>,
    connection: &Connection<'_>,
    generation: i64,
    file: File<'_>,
    parser: &mut ParseSession,
    cancel: &Cancellation,
) -> Result<()> {
    let File { id, path } = file;
    let status: String =
        connection.query_row("SELECT status FROM files WHERE id=?1", [id], |r| r.get(0))?;
    if status == "skipped" {
        return Err(Error::new(
            ErrorKind::Limit,
            "referenced configuration exceeds the input limit",
        ));
    }
    if status != "unsupported" {
        return Err(Error::operation(
            "referenced configuration is already classified as a different input kind",
        ));
    }
    let entry = inputs.get(path)?;
    let source = entry.read(selected.settings.max_file_bytes, cancel)?;
    if source.len() > selected.settings.max_file_bytes {
        return Err(Error::new(
            ErrorKind::Limit,
            "referenced configuration exceeds max_file_bytes",
        ));
    }
    let (package, module) = module_identity(
        &selected.root,
        path,
        Some(Language::TypeScript),
        &package_path(&entry, Some(Language::TypeScript))?,
    );
    let record = index::FileRecord {
        path: path.to_path_buf(),
        language: "auxiliary".into(),
        module,
        package,
        content_hash: entry.content_hash().to_owned(),
        size: source.len() as u64,
        status: "auxiliary",
    };
    record.persist(connection, generation)?;
    index::clear_file(connection, id)?;
    let mut context = index::FileFacts::new(id, &record, false);
    let mut writer = index::FactWriter::new(connection)?;
    formats::extract_configuration(parser, path, &source, cancel, &mut |language, fact| {
        cancel.check()?;
        if fact.owned_bytes() > selected.settings.fact_batch_bytes {
            return Err(Error::new(
                ErrorKind::Limit,
                "configuration fact exceeds fact_batch_bytes",
            ));
        }
        writer.persist(&mut context, language, fact)
    })
}
