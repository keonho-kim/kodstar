// Purpose: Scala given 소유·generic shadowing·타입 형식과 매개변수 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn scala_flat_and_variant_type_parameters_shadow_same_named_global_types() -> TestResult {
    let p = Project::new()?;
    p.write("Main.scala","class T\ntrait Show[A]\nclass Base\nclass Box[+T <: Base : Show](val value: T)\ndef use[T: Show](value: T): T = value\n")?;
    p.build(false)?;
    for name in ["Box", "use"] {
        let rows = p.query(Query::Deps {
            selector: selector("Main.scala", name),
        })?;
        let targets = relationships(&rows)
            .iter()
            .filter(|e| e.relation == "uses_type")
            .filter_map(|e| e.target.as_ref())
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>();
        assert!(targets.contains(&"Show"), "{:?}", rows.rows);
        assert!(!targets.contains(&"T"), "{:?}", rows.rows);
    }
    Ok(())
}

#[test]
fn scala_anonymous_givens_have_distinct_type_derived_owners() -> TestResult {
    let p = Project::new()?;
    p.write("Main.scala","trait Show[A]\ndef target() {}\ngiven Show[Int] with { def show(value: Int): String = { target(); \"\" } }\ngiven Show[String] with { def show(value: String): String = value }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.scala", "given Show[Int]::show"),
    })?;
    assert!(relationships(&rows).iter().any(|e|e.relation=="calls" && e.target.as_ref().is_some_and(|t|t.name=="target")),"{:?}",rows.rows);
    let other = p.query(Query::Deps {
        selector: selector("Main.scala", "given Show[String]::show"),
    })?;
    assert!(
        !relationships(&other)
            .iter()
            .any(|e| e.target.as_ref().is_some_and(|t| t.name == "target"))
    );
    Ok(())
}

#[test]
fn scala_all_parameter_lists_bind_receivers_and_preserve_signatures() -> TestResult {
    let p = Project::new()?;
    p.write("Main.scala","class Worker { def run() {} }\ndef caller[T](first: T)(using worker: Worker): Worker = { worker.run(); worker }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.scala", "caller"),
    })?;
    assert!(
        relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.target
                .as_ref()
                .is_some_and(|t| t.qualified_name == "Worker::run")),
        "{:?}",
        rows.rows
    );
    let explain = p.query(Query::Explain {
        selector: selector("Main.scala", "caller"),
    })?;
    assert!(explain.rows.iter().any(|row|matches!(row,QueryRow::Symbol(s) if s.signature.contains("worker:Worker") && s.signature.contains("first:T"))),"{:?}",explain.rows);
    Ok(())
}

#[test]
fn scala_type_aliases_own_match_types_and_qualified_names_do_not_fall_back_to_bare_types()
-> TestResult {
    let p = Project::new()?;
    p.write("Main.scala","class X\nclass Input\nclass Output\nclass Member\ntype Elem[X] = X match { case Input => Output }\ntype Foreign = missing.Member\ntype Projected[T] = T#Member\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.scala", "Elem"),
    })?;
    let targets = relationships(&rows)
        .iter()
        .filter_map(|e| e.target.as_ref())
        .map(|t| t.name.as_str())
        .collect::<Vec<_>>();
    assert!(
        targets.contains(&"Input") && targets.contains(&"Output"),
        "{:?}",
        rows.rows
    );
    assert!(!targets.contains(&"X"));
    for alias in ["Foreign", "Projected"] {
        let rows = p.query(Query::Deps {
            selector: selector("Main.scala", alias),
        })?;
        assert!(
            !relationships(&rows)
                .iter()
                .any(|e| e.target.as_ref().is_some_and(|t| t.name == "Member")),
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn scala_local_factories_and_constructor_fields_preserve_nominal_receiver_types() -> TestResult {
    let p = Project::new()?;
    p.write("Main.scala","class Worker { def run() {} }\nclass Holder(val worker: Worker)\ndef make(): Worker = new Worker()\ndef caller(holder: Holder) { val local=make(); local.run(); holder.worker.run(); val direct=new Worker(); direct.run() }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.scala", "caller"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e.relation == "calls"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == "Worker::run"))
            .map(|e| e.count)
            .sum::<u64>(),
        3,
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn scala_explicit_bases_keep_inheritance_mixin_and_self_type_distinct() -> TestResult {
    let p = Project::new()?;
    p.write("Main.scala","class Base\ntrait Mixin\ntrait Logging\nclass Child extends Base with Mixin { self: Logging => }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.scala", "Child"),
    })?;
    for (target, relation) in [
        ("Base", "inherits"),
        ("Mixin", "mixes_in"),
        ("Logging", "depends_on"),
    ] {
        assert!(
            relationships(&rows)
                .iter()
                .any(|e| e.relation == relation
                    && e.target.as_ref().is_some_and(|t| t.name == target)),
            "{:?}",
            rows.rows
        );
    }
    assert!(!relationships(&rows).iter().any(
        |e| e.relation == "inherits" && e.target.as_ref().is_some_and(|t| t.name == "Logging")
    ));
    Ok(())
}

#[test]
fn scala_companions_keep_type_and_value_members_separate_in_either_order() -> TestResult {
    for reverse in [false, true] {
        let p = Project::new()?;
        let class = "class Worker { def instance() {} }";
        let object = "object Worker { def static() {}; def apply(): Worker = new Worker() }";
        let declarations = if reverse {
            format!("{object}\n{class}")
        } else {
            format!("{class}\n{object}")
        };
        p.write("Main.scala",&format!("{declarations}\ndef caller(value: Worker) {{ value.instance(); Worker.static(); Worker.instance(); val built=Worker(); built.instance() }}\n"))?;
        p.build(false)?;
        let rows = p.query(Query::Deps {
            selector: selector("Main.scala", "caller"),
        })?;
        let edges = relationships(&rows);
        assert_eq!(
            edges
                .iter()
                .filter(|e| e.relation == "calls"
                    && e.target.as_ref().is_some_and(|t| t.name == "instance"))
                .map(|e| e.count)
                .sum::<u64>(),
            2,
            "{:?}",
            rows.rows
        );
        assert!(
            edges
                .iter()
                .any(|e| e.relation == "calls"
                    && e.target.as_ref().is_some_and(|t| t.name == "static")),
            "{:?}",
            rows.rows
        );
        assert!(
            edges
                .iter()
                .any(|e| e.relation == "calls"
                    && e.target.as_ref().is_some_and(|t| t.name == "apply")),
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn scala_imported_companions_and_explicit_given_values_keep_their_owners() -> TestResult {
    let p = Project::new()?;
    p.write("Types.scala","package p\nclass Worker { def instance() {}; def inside(){ this.instance() } }\nobject Worker { def static() {}; def inside(){ this.static() } }\ntrait Show[A]\ngiven named: Show[Int] with { def show() {} }\n")?;
    p.write("Main.scala","package p\nimport p.Worker\nimport p.named\ndef caller(value: Worker) { value.instance(); Worker.static(); named.show() }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.scala", "caller"),
    })?;
    for name in ["instance", "static", "show"] {
        assert!(
            relationships(&rows).iter().any(|e| e.relation == "calls"
                && e.resolution == "resolved"
                && e.target.as_ref().is_some_and(|t| t.name == name)),
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}
