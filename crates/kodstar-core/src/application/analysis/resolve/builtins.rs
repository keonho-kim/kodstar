// Purpose: 언어별 내장 이름과 타입의 외부 분류.
// Pattern: Simple Module.
// Usage: lexical lookup이 로컬·import를 찾지 못한 뒤 호출.
// Related: crates/kodstar-core/src/application/analysis/resolve/lexical.rs.

use crate::domain::languages::Language;

pub(super) fn builtin(language: Language, name: &str, relation: &str) -> bool {
    match language {
        Language::Rust => matches!(
            name,
            "String"
                | "str"
                | "Vec"
                | "Option"
                | "Result"
                | "Some"
                | "None"
                | "Ok"
                | "Err"
                | "Box"
                | "Self"
                | "usize"
                | "bool"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "i8"
                | "i16"
                | "i32"
                | "i64"
                | "println"
                | "print"
                | "eprintln"
                | "format"
                | "vec"
                | "assert"
                | "assert_eq"
        ),
        Language::Python => matches!(
            name,
            "print"
                | "len"
                | "range"
                | "str"
                | "int"
                | "float"
                | "bool"
                | "list"
                | "dict"
                | "set"
                | "tuple"
                | "object"
                | "Exception"
                | "super"
                | "getattr"
        ),
        Language::Go => {
            matches!(
                name,
                "append"
                    | "len"
                    | "cap"
                    | "make"
                    | "new"
                    | "delete"
                    | "panic"
                    | "recover"
                    | "copy"
                    | "close"
                    | "print"
                    | "println"
            ) || relation == "uses_type"
                && matches!(name, "int" | "string" | "bool" | "byte" | "error" | "any")
        }
        _ => false,
    }
}
