// Purpose: 분리 메서드·receiver와 generic 이름의 오연결 회귀 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn rust_imported_impl_owner_and_self_calls_survive_incremental_updates() -> TestResult {
    let project = Project::new()?;
    project.write("src/lib.rs", "mod api; mod implementation;")?;
    project.write("src/api.rs", "pub struct Service;")?;
    project.write("src/implementation.rs", "use crate::api::Service as Owner; impl Owner { fn target() {} fn caller(&self) { self.target(); Self::target(); } }")?;
    project.build(false)?;
    let query = || Query::Deps {
        selector: selector("src/implementation.rs", "Owner::caller"),
    };
    let rows = project.query(query())?;
    assert_eq!(relationships(&rows).iter().map(|e| e.count).sum::<u64>(), 2);
    let symbols = project.query(Query::Symbols {
        file: "src/implementation.rs".into(),
    })?;
    let owner = project.query(Query::Symbols {
        file: "src/api.rs".into(),
    })?;
    let id = owner
        .rows
        .iter()
        .find_map(|r| match r {
            QueryRow::Symbol(s) if s.name == "Service" => Some(s.id),
            _ => None,
        })
        .ok_or("owner")?;
    assert!(
        symbols
            .rows
            .iter()
            .any(|r| matches!(r,QueryRow::Symbol(s) if s.name=="caller" && s.owner==Some(id)))
    );
    project.write("src/api.rs", "pub struct Other;")?;
    project.build(false)?;
    assert!(relationships(&project.query(query())?).is_empty());
    project.write("src/api.rs", "pub struct Service;")?;
    project.build(false)?;
    let incremental = project.query(query())?;
    project.build(true)?;
    let full = project.query(query())?;
    assert_eq!(
        relationships(&incremental)
            .iter()
            .map(|e| e.count)
            .sum::<u64>(),
        relationships(&full).iter().map(|e| e.count).sum::<u64>()
    );
    Ok(())
}

#[test]
fn go_receivers_owned_across_files_and_composite_values_do_not_expose_bare_methods() -> TestResult {
    let project = Project::new()?;
    project.write(
        "types.go",
        "package api\ntype Service[T any] struct {}\ntype Other struct {}\n",
    )?;
    project.write("methods.go", "package api\nfunc (s *Service[T]) Target() {}\nfunc (s *Service[T]) Caller() { s.Target() }\n")?;
    project.write("main.go", "package api\nfunc caller(a, b *Service[int]) { v := Service[int]{}; a.Target(); b.Target(); v.Target(); Target() }\n")?;
    project.build(false)?;
    for (file, name, count) in [
        ("methods.go", "Service::Caller", 1),
        ("main.go", "caller", 3),
    ] {
        let rows = project.query(Query::Explain {
            selector: selector(file, name),
        })?;
        assert_eq!(
            relationships(&rows)
                .iter()
                .filter(|e| e.relation == "calls" && e.resolution == "resolved")
                .map(|e| e.count)
                .sum::<u64>(),
            count,
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn swift_extensions_attach_members_to_the_declared_type() -> TestResult {
    let project = Project::new()?;
    project.write("Type.swift", "class Service { func target() {} }")?;
    project.write(
        "Extension.swift",
        "extension Service { func caller() { self.target() } }",
    )?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("Extension.swift", "Service::caller"),
    })?;
    assert_eq!(relationships(&rows).len(), 1);
    assert_eq!(
        relationships(&rows)[0]
            .target
            .as_ref()
            .ok_or("target")?
            .file,
        "Type.swift"
    );
    Ok(())
}
