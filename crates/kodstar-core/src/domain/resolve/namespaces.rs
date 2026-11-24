// Purpose: 언어별 binding과 사용 문맥의 타입·값·함수·macro 이름 공간 분류.
// Pattern: Simple Module.
// Usage: binding 저장과 lookup이 같은 공간 값을 사용.
// Related: crates/kodstar-core/src/infrastructure/index/write.rs, crates/kodstar-core/src/application/analysis/resolve/rust/mod.rs.

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Namespace {
    Any,
    Types,
    Values,
    Macros,
    Functions,
    Packages,
}

impl Namespace {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Types => "type",
            Self::Values => "value",
            Self::Macros => "macro",
            Self::Functions => "function",
            Self::Packages => "package",
        }
    }

    pub(crate) fn binding(
        language: crate::domain::languages::Language,
        kind: &str,
        name: &str,
    ) -> Self {
        use crate::domain::languages::Language;
        if language == Language::Erlang {
            return match kind {
                "type" | "type_parameter" => Self::Types,
                "function" => Self::Functions,
                "macro" => Self::Macros,
                _ => Self::Any,
            };
        }
        if language == Language::Scala {
            return match kind {
                "type" | "type_parameter" => Self::Types,
                "function" | "local" | "parameter" | "parameter_default" | "field" | "value"
                | "call_result" | "scala_object" | "scala_given" | "alias" | "scala_self" => {
                    Self::Values
                }
                _ => Self::Any,
            };
        }
        if language == Language::Kotlin {
            return match kind {
                "type" | "type_parameter" => Self::Types,
                "function" | "local" | "parameter" | "parameter_default" | "field"
                | "call_result" | "value_object" | "value_constructor" | "kotlin_self" => {
                    Self::Values
                }
                _ => Self::Any,
            };
        }
        if language == Language::Php {
            return match kind {
                "function" | "php_method" | "php_import_function" => Self::Functions,
                "type" | "php_import_type" => Self::Types,
                "field" | "value" | "local" | "parameter" | "parameter_default" | "alias"
                | "call_result" | "php_import_value" => Self::Values,
                _ => Self::Any,
            };
        }
        if language == Language::CommonLisp {
            return match kind {
                "function" | "lisp_local_function" | "lisp_global_function" => Self::Functions,
                "type" | "type_parameter" | "lisp_global_type" => Self::Types,
                "field" | "value" | "local" | "parameter" | "parameter_default" | "alias" => {
                    Self::Values
                }
                "lisp_global_value" => Self::Values,
                "lisp_compiler_macro" | "lisp_setf_expander" => Self::Macros,
                "lisp_package_definition" | "lisp_scope_package" | "lisp_pkg_def" => Self::Packages,
                _ => Self::Any,
            };
        }
        if language != Language::Rust {
            return Self::Any;
        }

        match kind {
            "type" | "type_parameter" => Self::Types,
            "alias" if name == "Self" => Self::Types,
            "macro" => Self::Macros,
            "function"
            | "local"
            | "parameter"
            | "parameter_default"
            | "const_parameter"
            | "field"
            | "value"
            | "rust_constructor"
            | "value_constructor"
            | "rust_self_constructor" => Self::Values,
            _ => Self::Any,
        }
    }

    pub(crate) fn usage(
        language: crate::domain::languages::Language,
        relation: &str,
        expression: &str,
    ) -> Self {
        use crate::domain::languages::Language;
        if language == Language::Erlang {
            return if expression.ends_with('!') {
                Self::Macros
            } else {
                match relation {
                    "calls" | "references" | "indirect_call" => Self::Functions,
                    "uses_type" => Self::Types,
                    _ => Self::Any,
                }
            };
        }
        if language == Language::Scala {
            return match relation {
                "uses_type" | "inherits" | "implements" | "mixes_in" | "depends_on" => Self::Types,
                "calls" | "references" => Self::Values,
                _ => Self::Any,
            };
        }
        if language == Language::Kotlin {
            return match relation {
                "uses_type" | "inherits" | "implements" | "mixes_in" => Self::Types,
                "calls" | "references" => Self::Values,
                _ => Self::Any,
            };
        }
        if language == Language::Php {
            return match relation {
                "calls" => Self::Functions,
                "uses_type" | "inherits" | "implements" | "mixes_in" => Self::Types,
                "references" => Self::Values,
                _ => Self::Any,
            };
        }
        if language == Language::CommonLisp {
            return match relation {
                "calls" | "references" => Self::Functions,
                "uses_type" | "inherits" | "implements" | "mixes_in" => Self::Types,
                _ => Self::Any,
            };
        }
        if language != Language::Rust {
            return Self::Any;
        }

        if expression.ends_with('!') {
            return Self::Macros;
        }
        match relation {
            "uses_type" | "inherits" | "implements" | "mixes_in" => Self::Types,
            "calls" | "references" => Self::Values,
            _ => Self::Any,
        }
    }
}
