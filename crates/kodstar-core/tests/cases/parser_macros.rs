// Purpose: C/C++의 불투명 매크로 경계에서 정의·본문·미확정 관계를 보존하는 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트의 native 매크로 사례.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/src/infrastructure/parse/input/native.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn included_lowercase_macros_require_a_definition_on_every_update() -> TestResult {
    let p = Project::new()?;
    p.write(
        "checks.h",
        "#define check(value) do {} while (0);\n#define finish() do {} while (0);\n",
    )?;
    p.write("main.c", "#if TESTING\n#include \"checks.h\"\nvoid caller(void) {\n check(1)\n finish()\n}\n#endif\n")?;
    p.build(false)?;
    p.write("checks.h", "/* definitions removed */\n")?;
    assert!(p.build(false)?.coverage.partial_files > 0);
    let preserved = p.query(Query::Symbols {
        file: "main.c".into(),
    })?;
    assert!(
        preserved
            .rows
            .iter()
            .any(|row| matches!(row, QueryRow::Symbol(symbol) if symbol.name == "caller"))
    );
    p.write("unrelated.h", "#define check(value)\n#define finish()\n")?;
    assert!(p.build(false)?.coverage.partial_files > 0);
    p.write("checks.h", "#define check(value)\n#define finish()\n")?;
    p.build(false)?;
    p.build(true)?;
    Ok(())
}

#[test]
fn repeated_type_generation_macros_are_repaired_together() -> TestResult {
    let p = Project::new()?;
    let mut source = String::from("#define DEFINE_VALUE(type, value)\n");
    for _ in 0..20 {
        source.push_str("DEFINE_VALUE(unsigned char, '\\0');\n");
    }
    source.push_str("typedef REMOVE_CONST_(int) Raw;\n#define CHECK(value)\nvoid target();\nvoid caller() {\n CHECK(target())\n << \"message\";\n}\n");
    p.write("main.cpp", &source)?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "main.cpp".into(),
    })?;
    assert!(
        rows.rows
            .iter()
            .any(|r| matches!(r, QueryRow::Symbol(s) if s.name == "caller"))
    );
    Ok(())
}

#[test]
fn cpp_macro_blocks_keep_nested_call_evidence() -> TestResult {
    let p = Project::new()?;
    p.write(
        "main.cpp",
        r#"void target() {}
TEST_CASE("example") { SECTION("nested") { target(); } }
"#,
    )?;
    p.build(false)?;
    let mut rows = Collected::default();
    p.engine.query(
        &p.options(),
        &Query::Refs {
            selector: selector("main.cpp", "target"),
        },
        &QueryOptions {
            refresh: Refresh::None,
            include_candidates: true,
            ..QueryOptions::default()
        },
        &Cancellation::default(),
        &mut rows,
    )?;
    assert!(!relationships(&rows).is_empty());
    assert!(
        relationships(&rows)
            .iter()
            .all(|row| row.resolution != "resolved")
    );
    Ok(())
}

#[test]
fn native_declaration_and_repeated_statement_macros_keep_real_definitions() -> TestResult {
    let p = Project::new()?;
    p.write(
        "main.c",
        r#"#define check_cond(x) if (!(x)) abort();
int API_CALL main(void) {
 check_cond(1)
 check_cond(
 2)
 return 0;
}
"#,
    )?;
    p.write(
        "types.cpp",
        r#"#define API_CONSTEXPR constexpr
struct Node { template<typename T> explicit API_CONSTEXPR Node(T arg) : value(arg) {} int value; };
"#,
    )?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "main.c".into(),
    })?;
    assert!(
        rows.rows
            .iter()
            .any(|r| matches!(r, QueryRow::Symbol(s) if s.name == "main"))
    );
    Ok(())
}

#[test]
fn macro_class_headers_and_conditional_modifiers_keep_members() -> TestResult {
    let p = Project::new()?;
    p.write(
        "main.cpp",
        r#"BEGIN_NAMESPACE
BEGIN_EXPORT
class API_PUBLIC Value { public: Value() {} void work() { if API_CONSTEXPR (true) { target(); } if API_CONSTEXPR (true) return; } };
API_THREAD_LOCAL class { public: void work() { target(); } } instance;
WARNING_POP
template<typename T> struct Box { T value; };
API_PUBLIC struct Extra {};
template<typename T>
API_VISIBILITY("hidden") // parser annotation
API_CONSTEXPR auto parse(T value) -> T { return value; }
class API_VISIBILITY("default") Visible { public: void work() {} };
WARNING_PUSH("-Wexample")
Visible::size_type Visible::size() const { return size_; }
END_EXPORT
END_NAMESPACE
namespace inner {
#define BODY_IF_ENABLED(a, b) a
void body_macro() BODY_IF_ENABLED({}, ;)
struct Value { constexpr API_INLINE Value(unsigned char x API_BUILTIN) : value_(x) {} int value_; };
WARNING_PUSH("warn")
void caller() API_NOEXCEPT { auto fn = []() API_NOEXCEPT { target(); }; fn(); }
WARNING_POP
}
"#,
    )?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "main.cpp".into(),
    })?;
    for expected in [
        "Value::work",
        "Visible::work",
        "inner::body_macro",
        "inner::caller",
    ] {
        assert!(
            rows.rows
                .iter()
                .any(|r| matches!(r, QueryRow::Symbol(s) if s.qualified_name == expected)),
            "missing {expected}"
        );
    }
    Ok(())
}

#[test]
fn native_macros_in_type_parameters_and_class_members_are_opaque() -> TestResult {
    let p = Project::new()?;
    p.write("main.cpp", r#"#define REGISTER(Type, Name) struct Name {};
REGISTER(int, IntType);
struct Interface { DECLARE_INTERFACE(Interface)
 virtual void call() = 0; };
template<typename T, ENABLE_IF(!is_same<T, int>::value && limits<T>::signed_value)> void work(const REF_WRAP(T) value) { target(); }
"#)?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "main.cpp".into(),
    })?;
    assert!(
        rows.rows
            .iter()
            .any(|r| matches!(r, QueryRow::Symbol(s) if s.name == "work"))
    );
    Ok(())
}

#[test]
fn directive_comments_and_string_macros_keep_following_definitions() -> TestResult {
    let p = Project::new()?;
    p.write(
        "main.cpp",
        r#"#define CHECK(value) ((value) /* annotation */ \
 ? true : false)
#define PREFIX "prefix"
void target() {}
void caller() { auto text = "before" PREFIX "after"; target(); }
"#,
    )?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "main.cpp".into(),
    })?;
    assert!(
        rows.rows
            .iter()
            .any(|r| matches!(r, QueryRow::Symbol(s) if s.name == "caller"))
    );
    Ok(())
}

#[test]
fn malformed_macro_arguments_do_not_hide_valid_calls_or_enum_members() -> TestResult {
    let p = Project::new()?;
    p.write(
        "main.cpp",
        r#"template API_PUBLIC void Box<char>::append(const char*, const char*);
class EMPTY_BASE Mock : public Base {
  MOCK_METHOD(int, work, (int));
  int ordinary() NO_INLINE_;
  bool locked() REQUIRES_LOCK(mutex);
  enum Mode { FIRST_MODE, LAST_MODE };
  Mock() : first_(0), WARNING_PUSH(4355 /* annotation */) second_(1), WARNING_POP() third_(2) {}
  int first_, second_, third_;
};
void target() {}
TEST_F(Suite, example) {
  EXPECT_THROW(throw Error(), Error);
  EXPECT_THROW_MSG(throw Error(), Error, "text") << "detail";
  EXPECT_THROW_MSG({ throw_error(); }, Error, (1, "message"));
  target();
}
"#,
    )?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "main.cpp".into(),
    })?;
    assert!(
        rows.rows
            .iter()
            .any(|r| matches!(r, QueryRow::Symbol(s) if s.name == "LAST_MODE"))
    );
    Ok(())
}

#[test]
fn global_macro_functions_preserve_calls_and_class_constructors() -> TestResult {
    let p = Project::new()?;
    p.write(
        "main.cpp",
        r#"void target() {}
namespace Tests {
TEST_F(Suite, Plain) { target(); }
TEST(Suite, Base) { target(); }
END_EXPORT
TEST(Suite, Next) { target(); }
TEST(Suite, enum) { target(); }
}
class VALUE { public: VALUE() {} };
"#,
    )?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "main.cpp".into(),
    })?;
    assert!(
        !rows
            .rows
            .iter()
            .any(|r| matches!(r, QueryRow::Symbol(s) if s.name == "TEST_F" || s.name == "TEST"))
    );
    assert!(
        rows.rows
            .iter()
            .any(|r| matches!(r, QueryRow::Symbol(s) if s.qualified_name == "VALUE::VALUE"))
    );
    Ok(())
}
