// Purpose: 실제 프로젝트에서 확인한 문자열·reader·인코딩 입력의 구문과 원본 위치 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 입력 호환성과 오류 rollback을 검증.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/src/infrastructure/parse/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn jsx_attribute_urls_keep_literal_ampersands_and_call_sites() -> TestResult {
    for file in ["main.jsx", "main.tsx"] {
        let p = Project::new()?;
        let source = "function target() {}\nfunction caller() { target(); return <img src=\"https://example.test/?a=1&b=2&amp;c=3\" />; }\n";
        p.write(file, source)?;
        p.build(false)?;
        let rows = p.query(Query::Deps {
            selector: selector(file, "caller"),
        })?;
        assert!(
            relationships(&rows)
                .iter()
                .any(|r| r.target.as_ref().is_some_and(|t| t.name == "target"))
        );
    }
    Ok(())
}

#[test]
fn kotlin_multi_dollar_strings_keep_only_active_interpolation_calls() -> TestResult {
    let p = Project::new()?;
    p.write(
        "main.kt",
        "fun target() = 1\nfun caller() { val text = $$\"literal $ignored and $${target()}\" }\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.kt", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|r| r.target.as_ref().is_some_and(|t| t.name == "target"))
    );
    assert!(
        !relationships(&rows)
            .iter()
            .any(|r| r.expression.contains("ignored"))
    );
    Ok(())
}

#[test]
fn kotlin_multiline_primary_constructor_remains_owned_by_its_class() -> TestResult {
    let p = Project::new()?;
    p.write("main.kt", "class Outer {\n class Options\n private constructor(\n internal val value: String,\n ) { fun getValueAgain() = value }\n}\n")?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "main.kt".into(),
    })?;
    assert!(rows.rows.iter().any(
        |r| matches!(r, QueryRow::Symbol(s) if s.qualified_name == "Outer::Options::getValueAgain")
    ));
    Ok(())
}

#[test]
fn kotlin_secondary_constructor_follows_a_property_without_losing_its_body() -> TestResult {
    let p = Project::new()?;
    p.write("main.kt", "fun target() {}\nclass Reader {\n private var tags: Map<Class<*>, Any>? = null\n protected constructor() { target() }\n}\n")?;
    p.build(false)?;
    let rows = p.query(Query::Refs {
        selector: selector("main.kt", "target"),
    })?;
    assert!(!relationships(&rows).is_empty());
    Ok(())
}

#[test]
fn common_lisp_loop_clauses_remain_reader_forms_with_nested_calls() -> TestResult {
    let p = Project::new()?;
    p.write("main.lisp", "(defun target (x) x)\n(defun caller () (loop named scan for item of-type fixnum from 3 downto 0 thereis (target item)))\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("main.lisp", "caller"),
    })?;
    assert!(relationships(&rows).iter().any(|r| {
        r.target
            .as_ref()
            .is_some_and(|t| t.name.eq_ignore_ascii_case("target"))
    }));
    Ok(())
}

#[test]
fn invalid_code_encoding_and_unclosed_syntax_publish_explicit_file_failures() -> TestResult {
    for source in [b"void bad\xff(void) {}".as_slice(), b"void broken( {"] {
        let p = Project::new()?;
        p.write("main.c", "void target(void) {}\n")?;
        let first = p.build(false)?;
        std::fs::write(p.root.join("main.c"), source)?;
        let partial = p.build(false)?;
        if std::str::from_utf8(source).is_err() {
            assert_eq!(partial.coverage.failed_files, 1);
        } else {
            assert_eq!(partial.coverage.partial_files, 1);
        }
        let rows = p.query(Query::Symbols {
            file: "main.c".into(),
        })?;
        assert_eq!(
            rows.metadata.ok_or("metadata")?.generation,
            Some(first.generation + 1)
        );
    }
    Ok(())
}
