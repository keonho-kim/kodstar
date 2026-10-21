// Purpose: 타입 위치·decorator·조건부 선언·enum 소유권 회귀 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn python_annotations_and_decorators_reference_the_declared_symbol() -> TestResult {
    let project = Project::new()?;
    project.write("main.py", "class Target: pass\ndef decorate(fn): return fn\n@decorate\ndef caller(value: Target) -> Target:\n    return value\n")?;
    project.build(false)?;
    let rows = project.query(Query::Deps {
        selector: selector("main.py", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.relation == "references"
                && e.target.as_ref().is_some_and(|t| t.name == "decorate"))
    );
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e.relation == "uses_type"
                && e.target.as_ref().is_some_and(|t| t.name == "Target"))
            .map(|e| e.count)
            .sum::<u64>(),
        2
    );
    Ok(())
}

#[test]
fn build_and_preprocessor_conditions_remain_visible_on_definitions() -> TestResult {
    for (file, source, condition) in [
        (
            "main.go",
            "//go:build linux && amd64\n\npackage api\nfunc target() {}\n",
            "linux && amd64",
        ),
        (
            "main.c",
            "#if FEATURE\nint target(void) { return 0; }\n#endif\n",
            "FEATURE",
        ),
    ] {
        let project = Project::new()?;
        project.write(file, source)?;
        project.build(false)?;
        let rows = project.query(Query::Symbols { file: file.into() })?;
        assert!(rows.rows.iter().any(|r| matches!(r, QueryRow::Symbol(s) if s.name=="target" && s.condition.as_ref().is_some_and(|c|c.contains(condition)))),"{file}: {:?}",rows.rows);
    }
    Ok(())
}

#[test]
fn enum_members_keep_original_names_and_explicit_owners() -> TestResult {
    for (file, source, owner, member) in [
        ("main.rs", "enum Color { Red, RED }", "Color", "Red"),
        ("main.cpp", "enum class Color { Red, RED };", "Color", "Red"),
        ("Main.java", "enum Color { Red, RED }", "Color", "Red"),
        ("main.cs", "enum Color { Red, RED }", "Color", "Red"),
        ("main.ts", "enum Color { Red, RED }", "Color", "Red"),
        (
            "main.php",
            "<?php enum Color { case Red; case RED; }",
            "Color",
            "Red",
        ),
        ("main.kt", "enum class Color { Red, RED }\n", "Color", "Red"),
        (
            "main.swift",
            "enum Color { case Red, RED }\n",
            "Color",
            "Red",
        ),
        (
            "main.scala",
            "enum Color { case Red, RED }\n",
            "Color",
            "Red",
        ),
        (
            "main.zig",
            "const Color = enum { Red, RED };",
            "Color",
            "Red",
        ),
        (
            "main.pas",
            "unit Main; interface type TColor = (Red, Green); implementation end.",
            "TColor",
            "Red",
        ),
    ] {
        let project = Project::new()?;
        project.write(file, source)?;
        project.build(false)?;
        let rows = project.query(Query::Symbols { file: file.into() })?;
        let id = rows
            .rows
            .iter()
            .find_map(|r| match r {
                QueryRow::Symbol(s) if s.name == owner => Some(s.id),
                _ => None,
            })
            .ok_or("enum owner")?;
        assert!(
            rows.rows
                .iter()
                .any(|r| matches!(r,QueryRow::Symbol(s) if s.name==member && s.owner==Some(id))),
            "{file}: {:?}",
            rows.rows
        );
        let mut deps = Collected::default();
        project.engine.query(
            &project.options(),
            &Query::Related {
                selector: selector(file, owner),
            },
            &QueryOptions {
                refresh: Refresh::None,
                relations: vec![RelationKind::Contains],
                ..QueryOptions::default()
            },
            &Cancellation::default(),
            &mut deps,
        )?;
        assert!(
            relationships(&deps)
                .iter()
                .any(|e| e.relation == "contains"
                    && e.target.as_ref().is_some_and(|s| s.name == member)),
            "{file}"
        );
    }
    Ok(())
}
