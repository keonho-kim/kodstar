// Purpose: JVM annotation class 인자와 TypeScript decorator 소유 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/src/infrastructure/parse/type_uses.rs.

use crate::support::*;
use kodstar_core::*;

fn has(rows: &Collected, target: &str, relation: &str) -> bool {
    relationships(rows).iter().any(|e| {
        e.relation == relation
            && e.resolution == "resolved"
            && e.target.as_ref().is_some_and(|t| t.name == target)
    })
}

#[test]
fn java_annotation_class_literals_keep_qualified_types_and_declaration_owners() -> TestResult {
    let p = Project::new()?;
    p.write(
        "model/Customer.java",
        "package model; public class Customer {}",
    )?;
    p.write(
        "app/Bind.java",
        "package app; public @interface Bind { Class<?>[] value(); }",
    )?;
    p.write("app/Service.java", "package app; import model.Customer; @Bind({Customer.class}) class Service { @Bind({model.Customer.class}) void run() {} }")?;
    p.build(false)?;
    for owner in ["Service", "Service::run"] {
        let rows = p.query(Query::Deps {
            selector: selector("app/Service.java", owner),
        })?;
        assert!(has(&rows, "Bind", "references"), "{owner}: {:?}", rows.rows);
        assert!(
            has(&rows, "Customer", "uses_type"),
            "{owner}: {:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn kotlin_annotation_class_arguments_reference_the_annotation_and_actual_type() -> TestResult {
    let p = Project::new()?;
    p.write("model/Customer.kt", "package model\nclass Customer\n")?;
    p.write(
        "app/Bind.kt",
        "package app\nannotation class Bind(val value: kotlin.reflect.KClass<*>)\n",
    )?;
    p.write("app/Service.kt", "package app\nimport model.Customer\nclass Service {\n @Bind(Customer::class)\n fun run() {}\n}\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("app/Service.kt", "Service::run"),
    })?;
    assert!(has(&rows, "Bind", "references"), "{:?}", rows.rows);
    assert!(has(&rows, "Customer", "uses_type"), "{:?}", rows.rows);
    Ok(())
}

#[test]
fn typescript_class_method_parameter_and_field_decorators_keep_their_owners() -> TestResult {
    let p = Project::new()?;
    p.write("decorators.ts", "export function bind(...args: any[]): any {}\nexport function inject(...args: any[]): any {}\n")?;
    p.write("service.ts", "import {bind as B, inject as I} from './decorators';\n@B export class Service {\n @I field: string;\n @B\n run(@I value: string) {}\n}\n")?;
    p.build(false)?;
    let class = p.query(Query::Deps {
        selector: selector("service.ts", "Service"),
    })?;
    assert!(has(&class, "bind", "references"), "{:?}", class.rows);
    assert!(has(&class, "inject", "references"), "{:?}", class.rows);
    let method = p.query(Query::Deps {
        selector: selector("service.ts", "Service::run"),
    })?;
    assert!(has(&method, "bind", "references"), "{:?}", method.rows);
    assert!(has(&method, "inject", "references"), "{:?}", method.rows);
    assert_eq!(
        relationships(&method)
            .iter()
            .filter(|e| e.relation == "references")
            .map(|e| e.count)
            .sum::<u64>(),
        2
    );
    Ok(())
}

#[test]
fn stacked_namespace_decorators_and_constructor_parameters_keep_exact_sites() -> TestResult {
    let p = Project::new()?;
    p.write("decorators.ts", "export function first(...args: any[]): any {}\nexport function second(...args: any[]): any {}\n")?;
    p.write("service.ts", "import * as D from './decorators';\nclass Service {\n @D.first\n @D.second()\n run() {}\n constructor(@D.first value: string) {}\n}\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("service.ts", "Service::run"),
    })?;
    assert!(has(&rows, "first", "references"), "{:?}", rows.rows);
    assert!(has(&rows, "second", "references"), "{:?}", rows.rows);
    let sites: Vec<_> = relationships(&rows)
        .iter()
        .filter(|e| e.relation == "references")
        .map(|e| e.site.span.line)
        .collect();
    assert_eq!(sites, [3, 4]);
    let rows = p.query(Query::Deps {
        selector: selector("service.ts", "Service::constructor"),
    })?;
    assert!(has(&rows, "first", "references"), "{:?}", rows.rows);
    assert!(
        relationships(&rows)
            .iter()
            .filter(|e| e.relation == "references")
            .all(|e| e.site.span.line == 6)
    );
    Ok(())
}

#[test]
fn kotlin_class_literal_aliases_keep_the_original_receiver_span() -> TestResult {
    let p = Project::new()?;
    p.write("model/Customer.kt", "package model\nclass Customer\n")?;
    p.write(
        "app/Bind.kt",
        "package app\nannotation class Bind(val value: kotlin.reflect.KClass<*>)\n",
    )?;
    let source = "package app\nimport model.Customer as Buyer\nclass Service {\n @Bind(Buyer::class)\n fun run() {}\n}\n";
    p.write("app/Service.kt", source)?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("app/Service.kt", "Service::run"),
    })?;
    let edge = relationships(&rows)
        .into_iter()
        .find(|e| {
            e.relation == "uses_type" && e.target.as_ref().is_some_and(|t| t.name == "Customer")
        })
        .ok_or("class argument")?;
    assert_eq!(
        &source[edge.site.span.start_byte..edge.site.span.end_byte],
        "Buyer"
    );
    assert_eq!(edge.expression, "Buyer");
    Ok(())
}
