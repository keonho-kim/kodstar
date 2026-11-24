// Purpose: 공유 체크포인트의 참조 설정을 사전 검사하고 파일 결과를 원자적으로 저장.
// Pattern: Input Adapter.
// Usage: configurations가 동일 snapshot·statement를 빌려 파일별 호출.
// Related: crates/kodstar-core/src/application/analysis/configurations.rs, crates/kodstar-core/src/infrastructure/history/inputs/mod.rs.

use super::module_identity::{module_identity, package_path};
use crate::application::workspace::SelectedProject;
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, ErrorKind, InputIssue, Result},
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
        content_hash: entry.content_hash(),
        size: entry.size(),
        status: "auxiliary",
    };
    if entry.size() > selected.settings.max_file_bytes as u64 {
        return reject(
            connection,
            id,
            record,
            Error::input(
                "configuration_invalid",
                "referenced configuration exceeds max_file_bytes",
                None,
            ),
        );
    }
    let source = entry.read(selected.settings.max_file_bytes, cancel)?;
    parser.begin_file(true);
    let checked = formats::extract_configuration(parser, path, &source, cancel, &mut |_, fact| {
        cancel.check()?;
        if fact.owned_bytes() > selected.settings.fact_batch_bytes {
            return Err(Error::new(
                ErrorKind::Limit,
                "configuration fact exceeds fact_batch_bytes",
            ));
        }
        Ok(())
    });
    if let Err(error) = checked {
        parser.end_file();
        return reject(connection, id, record, error);
    }
    record.persist(connection)?;
    index::clear_file(connection, id)?;
    let mut context = index::FileFacts::new(id, &record, false);
    let mut writer = index::FactWriter::new(connection)?;
    parser.replay();
    let result =
        formats::extract_configuration(parser, path, &source, cancel, &mut |language, fact| {
            cancel.check()?;
            writer.persist(&mut context, language, fact)
        });
    parser.end_file();
    result
}

fn reject(
    connection: &Connection<'_>,
    id: i64,
    record: index::FileRecord,
    error: Error,
) -> Result<()> {
    let issue = match error.input.clone() {
        Some(issue) => issue,
        None if error.kind == ErrorKind::Limit => InputIssue {
            code: "configuration_invalid",
            line: None,
        },
        None => return Err(error),
    };
    index::failures::replace(connection, record, issue, &error.to_string(), None)?;
    connection.execute("INSERT INTO diagnostics(file_id,code,message) VALUES(?1,'configuration_incomplete','configuration input is unavailable or classified as a different input kind')", [id])?;
    Ok(())
}
