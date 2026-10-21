// Purpose: Zig tagged union·error set과 정적 import projection 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn zig_tagged_unions_and_error_sets_keep_members_without_reclassifying_bare_unions() -> TestResult {
    let p = Project::new()?;
    p.write("main.zig","const Tagged = union(enum) { first: u32, second: void }; const Errors = error{ Bad, Worse }; const Bare = union { field: u32 }; const Named = union(Tag) { named: u32 }; const Tag = enum { named };")?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "main.zig".into(),
    })?;
    for name in [
        "Tagged::first",
        "Tagged::second",
        "Errors",
        "Errors::Bad",
        "Errors::Worse",
        "Named::named",
    ] {
        assert!(
            rows.rows
                .iter()
                .any(|r| matches!(r,QueryRow::Symbol(s) if s.qualified_name==name)),
            "{name}: {:?}",
            rows.rows
        );
    }
    let bare = p.query(Query::Deps {
        selector: selector("main.zig", "Bare"),
    })?;
    assert!(
        !relationships(&bare)
            .iter()
            .any(|e| e.relation == "contains"),
        "{:?}",
        bare.rows
    );
    Ok(())
}

#[test]
fn zig_import_field_projection_preserves_the_local_alias_and_target_owner() -> TestResult {
    let p = Project::new()?;
    p.write("api.zig","pub const Nested = struct { pub fn target() void {} }; pub const Other = struct { pub fn target() void {} };")?;
    p.write(
        "main.zig",
        "const Alias = @import(\"api.zig\").Nested; fn caller() void { Alias.target(); }",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.zig", "caller"),
    })?;
    assert!(
        relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.resolution == "resolved"
            && e.target
                .as_ref()
                .is_some_and(|t| t.qualified_name == "Nested::target")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn zig_imports_respect_pub_for_containers_and_methods() -> TestResult {
    let p = Project::new()?;
    p.write("api.zig","const Hidden = struct { pub fn target() void {} }; pub const Visible = struct { fn hidden() void {} pub fn target() void {} };")?;
    p.write("main.zig","const api = @import(\"api.zig\"); fn caller() void { api.Hidden.target(); api.Visible.hidden(); api.Visible.target(); }")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.zig", "caller"),
    })?;
    for expression in ["api.Hidden.target", "api.Visible.hidden"] {
        assert!(
            relationships(&rows)
                .iter()
                .any(|e| e.expression == expression && e.resolution == "unresolved"),
            "{:?}",
            rows.rows
        );
    }
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "api.Visible.target" && e.resolution == "resolved"),
        "{:?}",
        rows.rows
    );
    Ok(())
}
