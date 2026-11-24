// Purpose: 언어별 scope 준비와 특수 선언의 적용 순서 선택.
// Pattern: Language Dispatch.
// Usage: 공통 AST 순회가 현재 언어의 구문 소유자에게 위임.
// Related: crates/kodstar-core/src/infrastructure/parse/declarations.rs, crates/kodstar-core/src/infrastructure/parse/nominal.rs.

use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::{Context, Extractor, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(super) fn language_context(
        &mut self,
        node: Node<'_>,
        context: &mut Context,
    ) -> Result<bool> {
        match self.language {
            Language::Python => self.python_comprehension(node, context)?,
            Language::Swift => self.swift_package(node)?,
            Language::C | Language::Cpp => {
                self.cxx_header_guard(node)?;
                self.cxx_macro(node, context)?;
            }
            Language::CommonLisp => {
                if !self.lisp_region(node, context)? {
                    return Ok(false);
                }
                self.lisp_scope(node, context)?;
            }
            Language::Ruby => self.ruby_context(node, context)?,
            _ => {}
        }
        Ok(true)
    }

    pub(super) fn special_definition(
        &self,
        node: Node<'_>,
    ) -> Result<Option<(String, SymbolKind)>> {
        let definition = match self.language {
            Language::Php => self.php_definition(node)?,
            Language::Ruby => self.ruby_definition(node)?,
            Language::Kotlin => self.kotlin_definition(node)?,
            Language::Swift => self.swift_definition(node)?,
            Language::Scala => self.scala_definition(node)?,
            Language::CommonLisp => {
                if let Some((name, kind)) = self.lisp_local_definition(node) {
                    Some((self.bounded_text(text(name, self.source))?, kind))
                } else {
                    self.lisp_global_definition(node)?
                }
            }
            Language::Erlang => match self.erlang_definition(node)? {
                Some(definition) => Some(definition),
                None => self.erlang_record_definition(node)?,
            },
            _ => None,
        };
        if definition.is_some() {
            return Ok(definition);
        }
        if let Some(definition) = self.namespace_definition(node)? {
            return Ok(Some(definition));
        }
        match self.language {
            Language::Rust => self.rust_definition(node),
            Language::C | Language::Cpp => self.cxx_definition(node),
            Language::Go | Language::Swift | Language::Kotlin => self.nominal_definition(node),
            Language::Zig => self.zig_definition(node),
            Language::Elixir => self.elixir_definition(node),
            Language::Erlang => self.erlang_function_definition(node),
            _ => Ok(None),
        }
    }
}
