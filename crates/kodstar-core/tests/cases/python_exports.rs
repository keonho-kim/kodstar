// Purpose: Python __all__·wildcard·모듈 재수출과 증분 무효화 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn python_all_controls_star_imports_but_not_explicit_imports() -> TestResult {
    let p = Project::new()?;
    p.write(
        "api.py",
        "__all__=['_allowed']\ndef public(): pass\ndef _allowed(): pass\ndef _hidden(): pass\n",
    )?;
    p.write(
        "main.py",
        "from api import *\ndef caller():\n    public()\n    _allowed()\n    _hidden()\n",
    )?;
    p.write(
        "explicit.py",
        "from api import public\ndef caller(): public()\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.py", "caller"),
    })?;
    let targets = relationships(&rows)
        .iter()
        .filter(|e| e.relation == "calls")
        .filter_map(|e| e.target.as_ref())
        .map(|t| t.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(targets, vec!["_allowed"], "{:?}", rows.rows);
    let rows = p.query(Query::Deps {
        selector: selector("explicit.py", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.target.as_ref().is_some_and(|t| t.name == "public"))
    );
    Ok(())
}

#[test]
fn python_dynamic_and_conditional_all_do_not_establish_firm_exports() -> TestResult {
    for exported in [
        "__all__=names()",
        "__all__=['target']\nif FLAG:\n    __all__=[]",
    ] {
        let p = Project::new()?;
        p.write("api.py", &format!("{exported}\ndef target(): pass\n"))?;
        p.write("main.py", "from api import *\ndef caller(): target()\n")?;
        p.build(false)?;
        let rows = p.query(Query::Explain {
            selector: selector("main.py", "caller"),
        })?;
        assert!(
            relationships(&rows).iter().any(|e| e.relation == "calls"
                && e.resolution == "candidate"
                && e.target.as_ref().is_some_and(|t| t.name == "target")),
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn python_static_concatenation_and_appends_preserve_private_export_names() -> TestResult {
    let p = Project::new()?;
    p.write("api.py","__all__=['first']+['second']\n__all__+=['_third']\ndef first(): pass\ndef second(): pass\ndef _third(): pass\ndef hidden(): pass\n")?;
    p.write(
        "main.py",
        "from api import *\ndef caller():\n    first()\n    second()\n    _third()\n    hidden()\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.py", "caller"),
    })?;
    let mut targets = relationships(&rows)
        .iter()
        .filter(|e| e.relation == "calls")
        .filter_map(|e| e.target.as_ref())
        .map(|t| t.name.as_str())
        .collect::<Vec<_>>();
    targets.sort();
    assert_eq!(
        targets,
        vec!["_third", "first", "second"],
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn python_reexported_import_aliases_follow_export_changes_without_reparsing_callers() -> TestResult
{
    let p = Project::new()?;
    p.write("actual.py", "def handler(): pass\n")?;
    p.write(
        "api.py",
        "from actual import handler as alias\n__all__=['alias']\n",
    )?;
    p.write("main.py", "from api import *\ndef caller(): alias()\n")?;
    p.build(false)?;
    let query = || Query::Deps {
        selector: selector("main.py", "caller"),
    };
    let rows = p.query(query())?;
    assert!(
        relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.target
                .as_ref()
                .is_some_and(|t| t.file == "actual.py" && t.name == "handler")),
        "{:?}",
        rows.rows
    );
    p.write(
        "api.py",
        "from actual import handler as alias\n__all__=[]\n",
    )?;
    assert_eq!(p.build(false)?.parsed_files, 1);
    assert!(relationships(&p.query(query())?).is_empty());
    p.build(true)?;
    assert!(relationships(&p.query(query())?).is_empty());
    Ok(())
}

#[test]
fn python_all_mutation_is_unknown_and_a_later_literal_reset_restores_static_exports() -> TestResult
{
    for assignment in [
        "__all__[0]='target'",
        "alias=__all__\nalias.append('target')",
        "mutate(__all__)",
        "__all__=[['target']]",
        "__all__='target'",
    ] {
        let p = Project::new()?;
        p.write(
            "api.py",
            &format!("__all__=['old']\n{assignment}\ndef target(): pass\n"),
        )?;
        p.write("main.py", "from api import *\ndef caller(): target()\n")?;
        p.build(false)?;
        let rows = p.query(Query::Explain {
            selector: selector("main.py", "caller"),
        })?;
        assert!(
            relationships(&rows).iter().any(|e| e.relation == "calls"
                && e.resolution == "candidate"
                && e.target.as_ref().is_some_and(|t| t.name == "target")),
            "{:?}",
            rows.rows
        );
    }
    let p = Project::new()?;
    p.write(
        "api.py",
        "__all__=names()\n__all__=['target']\ndef target(): pass\ndef other(): pass\n",
    )?;
    p.write(
        "main.py",
        "from api import *\ndef caller():\n    target()\n    other()\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.py", "caller"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e.relation == "calls")
            .count(),
        1,
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn python_module_attributes_do_not_implicitly_export_builtins() -> TestResult {
    let p = Project::new()?;
    p.write("api.py", "def local(): pass\n")?;
    p.write(
        "main.py",
        "from api import print as borrowed\ndef caller(): borrowed('text')\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.py", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.relation == "calls" && e.resolution == "unresolved"),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn python_qualified_candidates_from_multiple_module_files_are_preserved() -> TestResult {
    let p = Project::new()?;
    let source = "class Worker:\n    @staticmethod\n    def run(): pass\n";
    p.write("api.py", source)?;
    p.write("api/__init__.py", source)?;
    p.write(
        "main.py",
        "from api import Worker\ndef caller(): Worker.run()\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.py", "caller"),
    })?;
    let calls = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 2, "{:?}", rows.rows);
    assert!(calls.iter().all(|e| e.resolution == "ambiguous"));
    Ok(())
}
