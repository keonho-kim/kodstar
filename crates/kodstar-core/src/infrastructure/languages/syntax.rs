// Purpose: 언어별 선언·import·호출 노드 설정.
// Pattern: Simple Module.
// Usage: parser가 현재 언어의 구문 설정을 사용.
// Related: crates/kodstar-core/src/infrastructure/parse/mod.rs.

//! AST node sets adapted from Graphify 0.9.92, commit 86c57be8204673c2a1b9849a3292a7dd3ab373d8.
//! Language-specific binding and name semantics are handled by extraction hooks.

use crate::domain::languages::Language;

pub(crate) struct SyntaxConfig {
    pub classes: &'static [&'static str],
    pub functions: &'static [&'static str],
    pub imports: &'static [&'static str],
    pub calls: &'static [&'static str],
    pub call_field: &'static str,
}

impl SyntaxConfig {
    pub(crate) fn for_language(language: Language) -> Self {
        match language {
            Language::Python => Self {
                classes: &["class_definition"],
                functions: &["function_definition", "lambda"],
                imports: &["import_from_statement", "import_statement"],
                calls: &["call"],
                call_field: "function",
            },
            Language::JavaScript => Self {
                classes: &["class_declaration", "class"],
                functions: &[
                    "function_declaration",
                    "generator_function_declaration",
                    "method_definition",
                ],
                imports: &[
                    "export_statement",
                    "import_statement",
                    "variable_declarator",
                    "assignment_expression",
                ],
                calls: &[
                    "call_expression",
                    "jsx_opening_element",
                    "jsx_self_closing_element",
                    "new_expression",
                ],
                call_field: "function",
            },
            Language::TypeScript => Self {
                classes: &[
                    "abstract_class_declaration",
                    "class_declaration",
                    "class",
                    "enum_declaration",
                    "interface_declaration",
                    "type_alias_declaration",
                ],
                functions: &[
                    "abstract_method_signature",
                    "function_declaration",
                    "generator_function_declaration",
                    "method_definition",
                    "method_signature",
                ],
                imports: &[
                    "export_statement",
                    "import_statement",
                    "variable_declarator",
                    "assignment_expression",
                ],
                calls: &["call_expression", "new_expression"],
                call_field: "function",
            },
            Language::Java => Self {
                classes: &[
                    "annotation_type_declaration",
                    "class_declaration",
                    "enum_declaration",
                    "interface_declaration",
                    "record_declaration",
                ],
                functions: &["constructor_declaration", "method_declaration"],
                imports: &["import_declaration"],
                calls: &["method_invocation", "object_creation_expression"],
                call_field: "name",
            },
            Language::C => Self {
                classes: &["struct_specifier", "enum_specifier", "union_specifier"],
                functions: &["function_definition"],
                imports: &["preproc_include"],
                calls: &["call_expression"],
                call_field: "function",
            },
            Language::Cpp => Self {
                classes: &[
                    "namespace_definition",
                    "class_specifier",
                    "enum_specifier",
                    "struct_specifier",
                    "union_specifier",
                ],
                functions: &["function_definition"],
                imports: &["preproc_include"],
                calls: &["call_expression"],
                call_field: "function",
            },
            Language::Ruby => Self {
                classes: &["class", "module"],
                functions: &["method", "singleton_method"],
                imports: &["call"],
                calls: &["call"],
                call_field: "method",
            },
            Language::CSharp => Self {
                classes: &[
                    "class_declaration",
                    "enum_declaration",
                    "interface_declaration",
                    "record_declaration",
                    "struct_declaration",
                ],
                functions: &[
                    "method_declaration",
                    "constructor_declaration",
                    "local_function_statement",
                ],
                imports: &["using_directive"],
                calls: &["invocation_expression", "object_creation_expression"],
                call_field: "function",
            },
            Language::Kotlin => Self {
                classes: &["class_declaration", "object_declaration"],
                functions: &["function_declaration"],
                imports: &["import", "import_header"],
                calls: &["call_expression"],
                call_field: "",
            },
            Language::Scala => Self {
                classes: &[
                    "class_definition",
                    "enum_definition",
                    "given_definition",
                    "object_definition",
                    "trait_definition",
                ],
                functions: &["function_declaration", "function_definition"],
                imports: &["import_declaration"],
                calls: &["call_expression", "instance_expression"],
                call_field: "",
            },
            Language::Php => Self {
                classes: &[
                    "class_declaration",
                    "enum_declaration",
                    "interface_declaration",
                    "trait_declaration",
                ],
                functions: &[
                    "anonymous_function",
                    "arrow_function",
                    "function_definition",
                    "method_declaration",
                ],
                imports: &["namespace_use_clause"],
                calls: &[
                    "function_call_expression",
                    "member_call_expression",
                    "nullsafe_member_call_expression",
                    "object_creation_expression",
                    "scoped_call_expression",
                ],
                call_field: "function",
            },
            Language::Swift => Self {
                classes: &["class_declaration", "protocol_declaration"],
                functions: &[
                    "deinit_declaration",
                    "function_declaration",
                    "init_declaration",
                    "protocol_function_declaration",
                    "subscript_declaration",
                ],
                imports: &["import_declaration"],
                calls: &["call_expression"],
                call_field: "",
            },
            Language::Rust => Self {
                classes: &[
                    "struct_item",
                    "enum_item",
                    "trait_item",
                    "type_item",
                    "mod_item",
                ],
                functions: &[
                    "function_item",
                    "function_signature_item",
                    "macro_definition",
                ],
                imports: &["use_declaration", "extern_crate_declaration"],
                calls: &["call_expression", "macro_invocation"],
                call_field: "function",
            },
            Language::Go => Self {
                classes: &["type_spec"],
                functions: &["function_declaration", "method_declaration", "method_elem"],
                imports: &["import_declaration"],
                calls: &["call_expression"],
                call_field: "function",
            },
            Language::Zig => Self {
                classes: &[
                    "struct_declaration",
                    "enum_declaration",
                    "union_declaration",
                ],
                functions: &["function_declaration"],
                imports: &["variable_declaration"],
                calls: &["call_expression"],
                call_field: "function",
            },
            Language::Elixir => Self {
                classes: &[],
                functions: &[],
                imports: &["call"],
                calls: &["call"],
                call_field: "target",
            },
            Language::Erlang => Self {
                classes: &[],
                functions: &["fun_decl"],
                imports: &["import_attribute", "include_attribute"],
                calls: &["call"],
                call_field: "expr",
            },
            Language::CommonLisp => Self {
                classes: &[],
                functions: &[],
                imports: &[],
                calls: &["list_lit"],
                call_field: "function",
            },
            _ => Self {
                classes: &[],
                functions: &[],
                imports: &[],
                calls: &[],
                call_field: "function",
            },
        }
    }
}
