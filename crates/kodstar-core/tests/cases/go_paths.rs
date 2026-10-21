// Purpose: Go workspace·로컬 replacement·설정 무효화와 공개 경계 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn go_workspace_resolves_listed_modules_without_guessing_unlisted_siblings() -> TestResult {
    let p = Project::new()?;
    p.write("go.work", "go 1.25\nuse (\n ./app\n \"./shared lib\"\n)\n")?;
    p.write("app/go.mod", "module example.test/app\n")?;
    p.write("shared lib/go.mod", "module example.test/service\n")?;
    p.write("shared lib/api.go", "package service\nfunc Target() {}\n")?;
    p.write("unlisted/go.mod", "module example.test/other\n")?;
    p.write("unlisted/api.go", "package other\nfunc Target() {}\n")?;
    p.write("app/main.go","package main\nimport api \"example.test/service\"\nimport other \"example.test/other\"\nfunc caller(){ api.Target(); other.Target() }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("app/main.go", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "api.Target"
                && e.resolution == "resolved"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.file == "shared lib/api.go")),
        "{:?}",
        rows.rows
    );
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.expression == "other.Target" && e.target.is_some()),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn go_workspace_replacement_overrides_the_module_replacement() -> TestResult {
    let p = Project::new()?;
    p.write(
        "go.work",
        "go 1.25\nuse ./app\nreplace example.test/service => ./chosen\n",
    )?;
    p.write("app/go.mod","module example.test/app\nrequire example.test/service v1.0.0\nreplace example.test/service => ../other\n")?;
    for directory in ["chosen", "other"] {
        p.write(
            &format!("{directory}/go.mod"),
            "module example.test/service\n",
        )?;
        p.write(
            &format!("{directory}/api.go"),
            "package service\nfunc Target() {}\n",
        )?;
    }
    p.write(
        "app/main.go",
        "package main\nimport api \"example.test/service\"\nfunc caller(){ api.Target() }\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("app/main.go", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.target.as_ref().is_some_and(|t| t.file == "chosen/api.go")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn go_work_membership_change_invalidates_unmodified_callers() -> TestResult {
    let p = Project::new()?;
    p.write("go.work", "use (\n./app\n./lib\n)\n")?;
    p.write("app/go.mod", "module example.test/app\n")?;
    p.write("lib/go.mod", "module example.test/lib\n")?;
    p.write("lib/api.go", "package lib\nfunc Target() {}\n")?;
    p.write(
        "app/main.go",
        "package main\nimport api \"example.test/lib\"\nfunc caller(){ api.Target() }\n",
    )?;
    p.build(false)?;
    let query = Query::Deps {
        selector: selector("app/main.go", "caller"),
    };
    assert_eq!(relationships(&p.query(query.clone())?).len(), 1);
    p.write("go.work", "use ./app\n")?;
    assert_eq!(p.build(false)?.parsed_files, 1);
    assert!(relationships(&p.query(query.clone())?).is_empty());
    p.build(true)?;
    assert!(relationships(&p.query(query)?).is_empty());
    Ok(())
}

#[test]
fn go_imports_do_not_expose_unexported_functions() -> TestResult {
    let p = Project::new()?;
    p.write("go.mod", "module example.test/app\n")?;
    p.write(
        "lib/api.go",
        "package lib\nfunc hidden() {}\nfunc Public() {}\n",
    )?;
    p.write("main.go","package main\nimport api \"example.test/app/lib\"\nfunc caller(){ api.hidden(); api.Public() }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.go", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "api.hidden" && e.resolution == "unresolved"),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn go_manifest_byte_escapes_decode_utf8_paths_without_changing_names() -> TestResult {
    let p = Project::new()?;
    p.write("go.work", "use (\n./app\n\"./\\xC3\\xA9\"\n)\n")?;
    p.write("app/go.mod", "module example.test/app\n")?;
    p.write("é/go.mod", "module example.test/lib\n")?;
    p.write("é/api.go", "package lib\nfunc Target() {}\n")?;
    p.write(
        "app/main.go",
        "package main\nimport api \"example.test/lib\"\nfunc caller(){ api.Target() }\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("app/main.go", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.target.as_ref().is_some_and(|t| t.file == "é/api.go")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn go_version_specific_replacement_retains_selection_uncertainty() -> TestResult {
    let p = Project::new()?;
    p.write("app/go.mod","module example.test/app\nrequire example.test/lib v1.0.0\nreplace (\nexample.test/lib v2.0.0 => ../wrong\nexample.test/lib v1.0.0 => ../chosen\n)\n")?;
    for directory in ["chosen", "wrong"] {
        p.write(&format!("{directory}/go.mod"), "module example.test/lib\n")?;
        p.write(
            &format!("{directory}/api.go"),
            "package lib\nfunc Target() {}\n",
        )?;
    }
    p.write(
        "app/main.go",
        "package main\nimport api \"example.test/lib\"\nfunc caller(){ api.Target() }\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("app/main.go", "caller"),
    })?;
    assert!(
        relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.resolution == "candidate"
            && e.target.as_ref().is_some_and(|t| t.file == "chosen/api.go")),
        "{:?}",
        rows.rows
    );
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.target.as_ref().is_some_and(|t| t.file == "wrong/api.go")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn go_foreign_receiver_does_not_expose_private_methods() -> TestResult {
    let p = Project::new()?;
    p.write("go.mod", "module example.test/app\n")?;
    p.write(
        "lib/api.go",
        "package lib\ntype Item struct{}\nfunc (i Item) hidden() {}\nfunc (i Item) Public() {}\n",
    )?;
    p.write("main.go","package main\nimport api \"example.test/app/lib\"\nfunc caller(value api.Item){ value.hidden(); value.Public() }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.go", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "value.hidden" && e.resolution == "unresolved"),
        "{:?}",
        rows.rows
    );
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "value.Public" && e.resolution == "resolved"),
        "{:?}",
        rows.rows
    );
    Ok(())
}
