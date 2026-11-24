// Purpose: 정적 모듈의 성공·실패 lookup과 선언 변경을 선택적으로 갱신하는 계약 검증.
// Pattern: Differential Integration Test.
// Usage: engine이 언어별 모듈 변경과 전체 분석을 대조.
// Related: crates/kodstar-core/src/infrastructure/index/invalidation/static_modules.rs, crates/kodstar-core/tests/support/snapshot.rs.

use crate::support::{snapshot::normalized_index, *};
use kodstar_core::*;
use rusqlite::Connection;

#[test]
fn provider_changes_refresh_aliased_calls_and_match_full_analysis() -> TestResult {
    let project = Project::new()?;
    project.write(
        "caller.ts",
        "import { target as invoke } from './api'; function caller() { invoke(); }",
    )?;
    project.build(false)?;
    let caller = || selector("caller.ts", "caller");
    let initial = project.query(Query::Explain { selector: caller() })?;
    assert!(
        relationships(&initial)
            .iter()
            .any(|edge| edge.resolution == "unresolved")
    );

    for (source, resolved, compare_full) in [
        (Some("export function target() {}"), true, true),
        (Some("export function other() {}"), false, false),
        (Some("export function target() {}"), true, false),
        (None, false, true),
    ] {
        if let Some(source) = source {
            project.write("api.ts", source)?;
        } else {
            std::fs::remove_file(project.root.join("api.ts"))?;
        }
        let updated = project.build(false)?;
        assert_eq!(updated.parsed_files, u64::from(source.is_some()));
        assert_eq!(updated.coverage.reused_files, 1);
        let dependencies = project.query(Query::Deps { selector: caller() })?;
        assert_eq!(relationships(&dependencies).len(), usize::from(resolved));
        let explained = project.query(Query::Explain { selector: caller() })?;
        let calls = relationships(&explained);
        assert_eq!(calls.len(), 1, "{source:?}: {:?}", explained.rows);
        assert_eq!(calls[0].expression, "invoke");
        if resolved {
            assert_eq!(calls[0].resolution, "resolved");
            assert_eq!(calls[0].target.as_ref().ok_or("target")?.name, "target");
        } else {
            assert_eq!(calls[0].resolution, "unresolved");
        }
        if compare_full {
            assert_eq!(
                normalized_index(&updated)?,
                normalized_index(&project.build(true)?)?
            );
        }
    }
    Ok(())
}

#[test]
fn static_module_declarations_preserve_unrelated_files_and_refresh_missing_names() -> TestResult {
    for (extension, initial, changed, caller, stable) in [
        (
            "py",
            "def target(): pass\n",
            "def future(): pass\n",
            "from api import target, future\ndef caller():\n target()\n future()\n",
            "def local(): pass\ndef stable(): local()\n",
        ),
        (
            "js",
            "function target() {}",
            "export function future() {}",
            "import { target, future } from './api.js'; function caller() { target(); future(); }",
            "function local() {} function stable() { local(); }",
        ),
        (
            "ts",
            "function target() {}",
            "export function future() {}",
            "import { target, future } from './api'; function caller() { target(); future(); }",
            "function local() {} function stable() { local(); }",
        ),
        (
            "rs",
            "pub fn target() {}",
            "pub fn future() {}",
            "use crate::api::{target, future}; fn caller() { target(); future(); }",
            "fn local() {} fn stable() { local(); }",
        ),
        (
            "zig",
            "pub fn target() void {}",
            "pub fn future() void {}",
            "const api = @import(\"api.zig\"); fn caller() void { api.target(); api.future(); }",
            "fn local() void {} fn stable() void { local(); }",
        ),
    ] {
        let p = Project::new()?;
        let api = format!("api.{extension}");
        let other = format!("stable.{extension}");
        p.write(&api, initial)?;
        p.write(&format!("caller.{extension}"), caller)?;
        p.write(&other, stable)?;
        let original = p.build(false)?;
        let db = Connection::open(std::path::Path::new(&original.cache).join("index.sqlite"))?;
        packed_mutations::mark(&db, &other)?;
        postings::register(&db)?;
        let stable_id = |db: &Connection| {
            db.query_row("SELECT min(r.id) FROM relationships r
            JOIN facts f ON f.id=r.fact_id JOIN files source ON source.id=f.file_id WHERE source.path=?1",
            [other.as_bytes()], |r| r.get::<_,i64>(0))
        };
        let before = stable_id(&db)?;
        drop(db);
        p.write(&api, changed)?;
        let partial = p.build(false)?;
        let db = Connection::open(std::path::Path::new(&partial.cache).join("index.sqlite"))?;
        postings::register(&db)?;
        assert_eq!(
            stable_id(&db)?,
            before,
            "{extension}: unrelated graph was replaced"
        );
        drop(db);
        assert_eq!(
            normalized_index(&partial)?,
            normalized_index(&p.build(true)?)?,
            "{extension}"
        );
        std::fs::remove_file(p.root.join(&api))?;
        let partial = p.build(false)?;
        assert_eq!(
            normalized_index(&partial)?,
            normalized_index(&p.build(true)?)?,
            "{extension}: deletion"
        );
    }
    Ok(())
}

#[test]
fn higher_priority_javascript_candidates_invalidate_existing_imports() -> TestResult {
    let p = Project::new()?;
    p.write("api.js", "export function target() {}")?;
    p.write(
        "caller.ts",
        "import { target } from './api'; function caller() { target(); }",
    )?;
    p.write(
        "stable.js",
        "function local() {} function stable() { local(); }",
    )?;
    p.build(false)?;
    for source in [Some("export function target() {}"), None] {
        if let Some(source) = source {
            p.write("api.ts", source)?;
        } else {
            std::fs::remove_file(p.root.join("api.ts"))?;
        }
        let partial = p.build(false)?;
        assert_eq!(
            normalized_index(&partial)?,
            normalized_index(&p.build(true)?)?
        );
    }
    Ok(())
}
