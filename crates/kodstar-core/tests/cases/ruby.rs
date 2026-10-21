// Purpose: Ruby 정적 class factory·생성 receiver·singleton과 lookup barrier 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn ruby_external_singleton_owner_is_linked_without_fabricating_a_lexical_owner() -> TestResult {
    let p = Project::new()?;
    p.write("api.rb", "module Api\nend\n")?;
    p.write(
        "main.rb",
        "require_relative 'api'\ndef Api.target; end\ndef entry; Api.target; end\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.rb", "entry"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "Api.target"
                && e.resolution == "candidate"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == "Api::target")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn ruby_refinement_changes_reresolve_inherited_calls_and_match_full_analysis() -> TestResult {
    let p = Project::new()?;
    let source = "class Base\n def run; end\nend\nclass Child < Base\n def entry; run; end\nend\n";
    p.write("main.rb", source)?;
    p.build(false)?;
    let query = Query::Explain {
        selector: selector("main.rb", "Child::entry"),
    };
    assert!(relationships(&p.query(query.clone())?).iter().any(|e| {
        e.resolution == "resolved"
            && e.target
                .as_ref()
                .is_some_and(|t| t.qualified_name == "Base::run")
    }));
    p.write("main.rb", &(source.to_owned() + "using Unknown\n"))?;
    p.build(false)?;
    let incremental = p.query(query.clone())?;
    assert!(
        relationships(&incremental)
            .iter()
            .any(|e| e.resolution == "candidate"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == "Base::run")),
        "{:?}",
        incremental.rows
    );
    p.build(true)?;
    let full = p.query(query)?;
    let rows = |r: &Collected| {
        relationships(r)
            .into_iter()
            .map(|e| {
                (
                    e.expression.clone(),
                    e.resolution.clone(),
                    e.target.as_ref().map(|t| t.qualified_name.clone()),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(rows(&incremental), rows(&full));
    Ok(())
}

#[test]
fn ruby_custom_new_and_singleton_class_keep_construction_and_dispatch_separate() -> TestResult {
    let p = Project::new()?;
    p.write("main.rb","class Item\n def run; end\n def self.new; nil; end\n class << self\n def target; end\n end\nend\ndef entry\n value=Item.new\n value.run\n Item.target\nend\n")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.rb", "entry"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "value.run" && e.resolution == "unresolved"),
        "{:?}",
        rows.rows
    );
    assert!(
        relationships(&rows).iter().any(|e| e
            .target
            .as_ref()
            .is_some_and(|t| t.qualified_name == "Item::new")
            && e.resolution == "resolved"),
        "{:?}",
        rows.rows
    );
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "Item.target"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == "Item::target")
                && e.resolution == "resolved"),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn ruby_factories_define_namespaced_classes_and_preserve_block_method_owners() -> TestResult {
    let p = Project::new()?;
    p.write("main.rb","class Base; end\nmodule Billing\n Invoice = Class.new(Base) do\n def run; end\n end\n Record = Struct.new(:value) do\n def read; end\n end\n Entry = Data.define(:value)\nend\n")?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "main.rb".into(),
    })?;
    for name in [
        "Billing::Invoice",
        "Billing::Invoice::run",
        "Billing::Record",
        "Billing::Record::read",
        "Billing::Entry",
    ] {
        assert!(
            rows.rows
                .iter()
                .any(|r| matches!(r,QueryRow::Symbol(s) if s.qualified_name==name)),
            "{name}: {:?}",
            rows.rows
        );
    }
    let inherited = p.query(Query::Deps {
        selector: selector("main.rb", "Billing::Invoice"),
    })?;
    assert!(
        relationships(&inherited).iter().any(
            |e| e.relation == "inherits" && e.target.as_ref().is_some_and(|t| t.name == "Base")
        ),
        "{:?}",
        inherited.rows
    );
    Ok(())
}

#[test]
fn ruby_class_and_instance_sides_do_not_resolve_to_the_other_method() -> TestResult {
    let p = Project::new()?;
    p.write("main.rb","class Item\n def run; end\n def self.run; end\n def self.other; end\n def caller; self.run; end\nend\ndef entry\n value = Item.new\n value.run\n Item.run\n value.other\nend\n")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.rb", "entry"),
    })?;
    for expression in ["value.run", "Item.run"] {
        assert!(
            relationships(&rows)
                .iter()
                .any(|e| e.expression == expression
                    && e.resolution == "resolved"
                    && e.target.is_some()),
            "{:?}",
            rows.rows
        );
    }
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "value.other" && e.resolution == "unresolved"),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn ruby_mixin_and_refinement_barriers_do_not_claim_runtime_dispatch_certainty() -> TestResult {
    let p = Project::new()?;
    p.write("main.rb","module Feature\n def run; end\nend\nclass Item\n include Feature\n def caller; run; end\nend\n")?;
    p.build(false)?;
    let uses = p.query(Query::Deps {
        selector: selector("main.rb", "Item"),
    })?;
    assert!(
        relationships(&uses)
            .iter()
            .any(|e| e.relation == "mixes_in"
                && e.target.as_ref().is_some_and(|t| t.name == "Feature")),
        "{:?}",
        uses.rows
    );
    let rows = p.query(Query::Explain {
        selector: selector("main.rb", "Item::caller"),
    })?;
    assert!(
        relationships(&rows).iter().any(|e| e.relation == "calls"
            && e.resolution == "candidate"
            && e.target
                .as_ref()
                .is_some_and(|t| t.qualified_name == "Feature::run")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn ruby_extend_imports_instance_methods_into_the_singleton_side() -> TestResult {
    let p = Project::new()?;
    p.write("main.rb","module Feature\n def run; end\n def self.other; end\nend\nclass Item\n extend Feature\nend\ndef entry\n Item.run\n Item.other\nend\n")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.rb", "entry"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "Item.run"
                && e.resolution == "candidate"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == "Feature::run")),
        "{:?}",
        rows.rows
    );
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "Item.other" && e.resolution == "unresolved"),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn ruby_block_parameters_shadow_constructor_bindings_and_methods_do_not_capture_class_locals()
-> TestResult {
    let p = Project::new()?;
    p.write("main.rb","class Worker\n def run; end\nend\nclass Owner\n value = Worker.new\n def entry; value.run; end\nend\ndef caller\n value=Worker.new\n [1].each do |value|\n value.run\n end\n value.run\nend\n")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("main.rb", "caller"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e.expression == "value.run" && e.resolution == "resolved")
            .map(|e| e.count)
            .sum::<u64>(),
        1,
        "{:?}",
        rows.rows
    );
    let method = p.query(Query::Explain {
        selector: selector("main.rb", "Owner::entry"),
    })?;
    assert!(
        relationships(&method)
            .iter()
            .any(|e| e.expression == "value.run" && e.resolution == "unresolved"),
        "{:?}",
        method.rows
    );
    Ok(())
}
