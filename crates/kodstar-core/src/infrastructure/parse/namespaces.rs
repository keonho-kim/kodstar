// Purpose: block·file namespace의 명시적 scope와 후속 선언 소유 문맥.
// Pattern: Simple Module.
// Usage: 선언 순회와 특수 선언 처리가 namespace 노드를 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/forms.rs, crates/kodstar-core/src/application/analysis/resolve/namespaces.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Definition, Fact, Scope};
use crate::domain::languages::Language;
use crate::domain::source::SourceSpan;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::{Context, Extractor, NamePath, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(super) fn namespace_definition(
        &self,
        node: Node<'_>,
    ) -> Result<Option<(String, SymbolKind)>> {
        let block = matches!(
            (self.language, node.kind()),
            (Language::CSharp, "namespace_declaration") | (Language::Php, "namespace_definition")
        ) && node.child_by_field_name("body").is_some();
        if !block {
            return Ok(None);
        }
        let Some(name) = node.child_by_field_name("name") else {
            return Ok(None);
        };
        Ok(Some((
            self.bounded_text(text(name, self.source))?,
            SymbolKind::Module,
        )))
    }

    pub(super) fn namespace_context(
        &mut self,
        node: Node<'_>,
        context: &mut Context,
    ) -> Result<()> {
        if !node
            .parent()
            .is_some_and(|parent| parent.parent().is_none())
        {
            return Ok(());
        }
        let declaration = matches!(
            (self.language, node.kind()),
            (Language::CSharp, "file_scoped_namespace_declaration")
                | (Language::Php, "namespace_definition")
        ) && node.child_by_field_name("body").is_none();
        if declaration {
            let Some(name) = node.child_by_field_name("name") else {
                return Ok(());
            };
            let name = self.bounded_text(text(name, self.source))?;
            let qualified = name.replace(['.', '\\'], "::");
            let prefix = NamePath::root(&qualified, self.fact_limit)?;
            let id = self.allocate()?;
            let mut next = node.next_named_sibling();
            let mut end = self.source.len();
            while let Some(sibling) = next {
                if matches!(
                    sibling.kind(),
                    "namespace_definition" | "file_scoped_namespace_declaration"
                ) {
                    end = sibling.start_byte();
                    break;
                }
                next = sibling.next_named_sibling();
            }
            (self.emit)(Fact::Scope(Scope {
                local_id: id,
                parent: Some(0),
                span: SourceSpan::at(self.source, node.start_byte(), end),
            }))?;
            (self.emit)(Fact::Definition(Definition {
                local_id: id,
                owner: Some(0),
                scope: 0,
                name: name.clone(),
                qualified_name: qualified,
                kind: SymbolKind::Module,
                signature: String::new(),
                condition: None,
                span: span(node),
                exported: true,
            }))?;
            (self.emit)(Fact::Binding(Binding {
                scope: 0,
                name,
                target: Some(id),
                module: None,
                imported_name: None,
                type_name: None,
                kind: "type",
                span: span(node),
            }))?;
            self.file_namespace = Some(Context {
                scope: id,
                source: id,
                owner: id,
                prefix,
                ..Context::default()
            });
        }
        if let Some(namespace) = &self.file_namespace {
            *context = namespace.clone();
        }
        Ok(())
    }
}
