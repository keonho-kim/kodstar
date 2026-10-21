// Purpose: 정적 getattr의 builtin·receiver 근거와 accessor 비호출성 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn python_static_getattr_uses_the_known_receiver_and_literal_attribute() -> TestResult {
    let p = Project::new()?;
    p.write("main.py","class Worker:\n    def run(self): pass\ndef caller(value: Worker):\n    callback=getattr(value, 'run')\n")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.py", "caller"),
    })?;
    for relation in ["references", "indirect_call"] {
        assert!(
            relationships(&rows).iter().any(|e| e.relation == relation
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == "Worker::run")),
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn python_shadowed_getattr_and_unknown_receivers_do_not_guess_a_global_handler() -> TestResult {
    for source in [
        "class Worker:\n    def run(self): pass\ndef getattr(value,name): return 1\ndef caller(value: Worker): return getattr(value,'run')\n",
        "def run(): pass\ndef caller(value): return getattr(value,'run')\n",
        "class Worker:\n    def run(self): pass\ndef caller(value: Worker,name): return getattr(value,name)\n",
    ] {
        let p = Project::new()?;
        p.write("main.py", source)?;
        p.build(false)?;
        let rows = p.query(Query::Explain {
            selector: selector("main.py", "caller"),
        })?;
        assert!(
            !relationships(&rows).iter().any(|e| matches!(
                e.relation.as_str(),
                "references" | "indirect_call"
            ) && e
                .target
                .as_ref()
                .is_some_and(|t| t.name == "run")),
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn property_and_accessor_values_are_not_inferred_to_be_the_getter_function() -> TestResult {
    for (file, source) in [
        (
            "main.py",
            "class Worker:\n    @property\n    def callback(self) -> int: return 1\ndef caller(value: Worker): return getattr(value,'callback')\n",
        ),
        (
            "main.ts",
            "class Worker { get callback(): number {return 1;} } function submit(value: unknown){} function caller(value: Worker){const callback=value.callback; submit(callback);}",
        ),
    ] {
        let p = Project::new()?;
        p.write(file, source)?;
        p.build(false)?;
        let rows = p.query(Query::Explain {
            selector: selector(file, "caller"),
        })?;
        assert!(
            !relationships(&rows)
                .iter()
                .any(|e| e.relation == "indirect_call"
                    && e.target.as_ref().is_some_and(|t| t.name == "callback")),
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn python_getattr_follows_known_call_results_and_indexed_module_aliases() -> TestResult {
    let p = Project::new()?;
    p.write("service.py", "def handler(): pass\n")?;
    p.write("main.py","import service as api\nclass Worker:\n    def run(self): pass\ndef make() -> Worker: return Worker()\ndef caller():\n    a=getattr(make(),'run')\n    b=getattr(api,'handler')\n")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.py", "caller"),
    })?;
    for name in ["run", "handler"] {
        assert!(
            relationships(&rows)
                .iter()
                .any(|e| e.relation == "indirect_call"
                    && e.resolution == "candidate"
                    && e.target.as_ref().is_some_and(|t| t.name == name)),
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn wildcard_getattr_origin_is_checked_after_the_export_list() -> TestResult {
    for exported in [true, false] {
        let p = Project::new()?;
        p.write(
            "custom.py",
            if exported {
                "def getattr(value,name): return 1\n"
            } else {
                "__all__=[]\ndef getattr(value,name): return 1\n"
            },
        )?;
        p.write("main.py","from custom import *\nclass Worker:\n    def run(self): pass\ndef caller(value: Worker): return getattr(value,'run')\n")?;
        p.build(false)?;
        let rows = p.query(Query::Explain {
            selector: selector("main.py", "caller"),
        })?;
        assert_eq!(
            relationships(&rows)
                .iter()
                .any(|e| e.relation == "indirect_call"
                    && e.target.as_ref().is_some_and(|t| t.name == "run")),
            !exported,
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}
