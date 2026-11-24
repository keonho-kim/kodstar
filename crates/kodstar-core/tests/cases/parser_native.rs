// Purpose: 전처리 조각과 최신 C#·Swift 구문에서 정의와 호출을 보존하는 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트의 native 문법 사례.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/src/infrastructure/parse/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn c_variadic_type_arguments_and_calling_convention_macros_parse() -> TestResult {
    let p = Project::new()?;
    p.write("casts.c", "struct STORAGE { int field; };\nstruct STORAGE value;\nint API_CALL entry(void)\n{\n ATTRIBUTE const unsigned char* bytes = (ATTRIBUTE const unsigned char*)0;\n return 0;\n}\n")?;
    p.write("main.c", "#include <stdarg.h>\n#define API_CALL\nstruct Hooks { void *(API_CALL *allocate)(int size); };\nvoid *caller(va_list args) { return va_arg(args, char *); }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "main.c".into(),
    })?;
    assert!(
        rows.rows
            .iter()
            .any(|r| matches!(r, QueryRow::Symbol(s) if s.name == "caller"))
    );
    Ok(())
}

#[test]
fn conditional_function_modifiers_keep_the_method_and_its_body() -> TestResult {
    let p = Project::new()?;
    p.write("Main.cs", "class Main { static void Target() {}\n#if FEATURE\n public\n#else\n private\n#endif\n static void Caller() { Target(); }\n}\n")?;
    p.build(false)?;
    let rows = p.query(Query::Refs {
        selector: selector("Main.cs", "Main::Target"),
    })?;
    assert_eq!(relationships(&rows).len(), 2);
    let definitions = p.query(Query::Symbols {
        file: "Main.cs".into(),
    })?;
    let variants: Vec<_> = definitions
        .rows
        .iter()
        .filter_map(|row| match row {
            QueryRow::Symbol(symbol) if symbol.name == "Caller" => Some(symbol),
            _ => None,
        })
        .collect();
    assert_eq!(variants.len(), 2);
    assert!(variants.iter().all(|symbol| symbol.condition.is_some()));
    Ok(())
}

#[test]
fn csharp_extension_blocks_preserve_member_definitions() -> TestResult {
    let p = Project::new()?;
    p.write("Main.cs", "static class Extensions { extension(string receiver) { public int Count() { return receiver.Length; } } }")?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "Main.cs".into(),
    })?;
    assert!(
        rows.rows
            .iter()
            .any(|r| matches!(r, QueryRow::Symbol(s) if s.name == "Count"))
    );
    Ok(())
}

#[test]
fn swift_open_ended_expression_patterns_preserve_the_case_body() -> TestResult {
    let p = Project::new()?;
    p.write("Main.swift", "func target() {}\nfunc caller(n: Int, value: Int) {\n switch value {\n case (n / 2 + 1)...: target()\n default: break\n }\n}\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.swift", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|r| r.target.as_ref().is_some_and(|t| t.name == "target"))
    );
    Ok(())
}

#[test]
fn swift_throwing_async_iteration_keeps_calls_after_the_loop() -> TestResult {
    let p = Project::new()?;
    p.write("Main.swift", "func target() {}\nfunc caller(stream: Stream) async throws {\n for try await line in try stream.lines { print(line) }\n target()\n}\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.swift", "caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|r| r.target.as_ref().is_some_and(|t| t.name == "target"))
    );
    Ok(())
}

#[test]
fn swift_isolation_modifiers_and_runtime_conditions_parse_in_their_contexts() -> TestResult {
    let p = Project::new()?;
    p.write("Main.swift", "#if _runtime(_multithreaded)\nfunc caller(body: nonisolated(nonsending) () async -> Void) {\n nonisolated(unsafe) var value = 1\n print(value)\n}\n#endif\n")?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "Main.swift".into(),
    })?;
    assert!(
        rows.rows
            .iter()
            .any(|r| matches!(r, QueryRow::Symbol(s) if s.name == "caller"))
    );
    Ok(())
}

#[test]
fn cpp_unevaluated_comma_pointer_parameters_and_nested_templates_parse() -> TestResult {
    let p = Project::new()?;
    p.write("main.cpp", "#if __has_include(<format>)\n#include <format>\n#endif\ntemplate<typename T> auto test(int) -> decltype(f(T{}), std::true_type{});\ntemplate<typename T, typename std::enable_if<T::value>::type * = nullptr> void work(T& value) { target(std::make_index_sequence<std::tuple_size_v<std::remove_reference_t<T>>>{}); target(typename T::value_type()); auto n = unsigned(1); }\nenum : unsigned { type_mask = 7 };\n")?;
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
fn missing_semicolons_remain_partial_and_private_macro_markers_are_rejected() -> TestResult {
    for source in [
        "void TARGET() {}\nvoid caller() {\n TARGET()\n}\n",
        "extern void TARGET();\nvoid caller() {\n TARGET()\n}\n",
        "void target();\nvoid caller() {\n target()\n}\n",
        "void caller() {\n missing()\n}\n",
        "@ACRO(x) { target(); }\n",
        "\u{1f}API int caller() {}\n",
    ] {
        let p = Project::new()?;
        p.write("main.cpp", source)?;
        let partial = p.build(false)?;
        if source.starts_with('@') || source.starts_with('\u{1f}') {
            assert_eq!(partial.coverage.failed_files, 1);
            assert_eq!(partial.coverage.partial_files, 0);
        }
        assert!(
            partial.coverage.failed_files
                + partial.coverage.partial_files
                + partial.coverage.fragment_files
                > 0
        );
    }
    Ok(())
}

#[test]
fn conditional_fragments_require_coverage_in_a_valid_projection() -> TestResult {
    let p = Project::new()?;
    p.write("main.c", "#if HOST\ntypedef\n#if RUNTIME\nint\n#else\nlong\n#endif\nProvider;\n#else\ntypedef int Provider;\n#endif\nProvider value;\n")?;
    p.build(false)?;
    p.write(
        "broken.c",
        "#if FEATURE\nint broken( {\n#else\nvoid good() {}\n#endif\n",
    )?;
    assert_eq!(p.build(false)?.coverage.partial_files, 1);
    let rows = p.query(Query::Symbols {
        file: "broken.c".into(),
    })?;
    assert!(
        rows.rows
            .iter()
            .any(|r| matches!(r, QueryRow::Symbol(s) if s.name == "good"))
    );
    Ok(())
}

#[test]
fn explicit_class_instantiation_accepts_pointer_to_member_template_arguments() -> TestResult {
    let p = Project::new()?;
    p.write(
        "main.cpp",
        r#"struct File {};
struct Buffer { File* pointer; };
struct Tag {};
template<typename T, typename B, File* B::* Member> class Access {};
template class Access<Tag, Buffer, &Buffer::pointer>;
template<typename T> struct IsNaN {};
extern template struct API_VISIBLE IsNaN<float>;
void caller() {}
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
fn cpp_type_defaults_using_typename_member_pointer_and_enum_attributes_parse() -> TestResult {
    let p = Project::new()?;
    p.write(
        "main.cpp",
        r#"template<bool ENABLED, typename A, typename P = const A*>
struct Box { using typename A::value_type; };
struct X { int value; int get() { return (*this).*(&X::value); } };
template<typename T> void removed(T) = delete;
template<typename C, typename F> struct Holder { const F C::*field_; int* pointer_; int values_[3]; void method(); };
enum class [[=tags::names]] Color { Red, Green };
"#,
    )?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "main.cpp".into(),
    })?;
    for (expected, kind) in [
        ("X::get", "method"),
        ("Holder::field_", "field"),
        ("Holder::pointer_", "field"),
        ("Holder::values_", "field"),
        ("Holder::method", "method"),
    ] {
        assert!(
            rows.rows
                .iter()
                .any(|r| matches!(r, QueryRow::Symbol(s) if s.qualified_name == expected && s.kind == kind)),
            "missing {expected}"
        );
    }
    Ok(())
}

#[test]
fn cpp_operator_templates_and_parameter_annotations_parse_in_their_context() -> TestResult {
    let p = Project::new()?;
    p.write(
        "main.cpp",
        r#"template<typename T> struct Functor {
 template<typename U> void operator()(U value) {}
 void caller(bool value) { operator()<bool>(value); (object->*method)(value); }
 ~Functor() override LOCK_EXCLUDED(mutex_) { target(); }
 virtual void pure() LOCK_REQUIRED(mutex_) = 0;
};
template<typename T> void annotated(const T& value LIFETIME_BOUND, int count) {}
typedef struct _CRITICAL_SECTION CRITICAL_SECTION_ALIAS;
std::string Getter::trace(int depth) LOCK_EXCLUDED(mutex_) { return target(); }
Getter::~Getter() LOCK_EXCLUDED(&Getter::mutex_) { target(); }
struct FilePath { bool exists() const; };
#define COMPARE_LE(left, right) le(left, right)
enum Comparison { eq = 0, ne, gt, lt, ge, le };
bool FilePath::exists() const { const FilePath& path(*this); return target(); }
FilePath declared(FilePath());
template<typename Class, typename T> Matcher<T (Class::*)() const> property(T (Class::*member)() const) { return target(); }
template<typename T1, class T2> using Bound = typename BIND_TYPES(T1, T2);
template<typename T> using Address = const REMOVE_CONST_(T) *;
class API_PUBLIC RE { public: explicit RE(int value) : value_(value) {} RE(const RE& other) { target(); } private: int value_; };
WARNING_POP_()
[[noreturn]] inline void Abort() { target(); }
template<typename T1, class T2> T1 typed(const T1& left, const T2& right) { return left; }
"#,
    )?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "main.cpp".into(),
    })?;
    for name in ["annotated", "typed", "Abort"] {
        assert!(
            rows.rows.iter().any(
                |r| matches!(r, QueryRow::Symbol(s) if s.name == name && s.kind == "function")
            ),
            "missing {name}"
        );
    }
    Ok(())
}
