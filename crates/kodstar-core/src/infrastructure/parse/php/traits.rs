// Purpose: PHP trait as·insteadof의 별칭·제외·접근 변경 사실 추출.
// Pattern: Simple Module.
// Usage: PHP 참조 추출기가 trait 적응 노드를 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/php/mod.rs, crates/kodstar-core/src/application/analysis/resolve/php/traits.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::infrastructure::parse::{Context, Extractor, child_kind, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn php_trait_rule(
        &mut self,
        node: Node<'_>,
        c: &Context,
    ) -> Result<()> {
        if !matches!(node.kind(), "use_as_clause" | "use_instead_of_clause") {
            return Ok(());
        }
        let Some(owner) = c.type_owner else {
            return Ok(());
        };
        let Some(reference) = node.named_child(0) else {
            return Ok(());
        };
        let (module, method) = if reference.kind() == "class_constant_access_expression" {
            let (Some(module), Some(method)) = (reference.named_child(0), reference.named_child(1))
            else {
                return Ok(());
            };
            (
                Some(self.reference_text(module)?),
                self.reference_text(method)?,
            )
        } else {
            (None, self.reference_text(reference)?)
        };
        if node.kind() == "use_as_clause" {
            let alias = node
                .named_child(node.named_child_count().saturating_sub(1) as u32)
                .filter(|n| n.id() != reference.id() && n.kind() == "name");
            let name = alias
                .map(|n| self.reference_text(n))
                .transpose()?
                .unwrap_or_else(|| method.clone());
            let visibility = child_kind(node, &["visibility_modifier"])
                .map(|n| self.bounded_text(text(n, self.source)))
                .transpose()?;
            (self.emit)(Fact::Binding(Binding {
                scope: c.scope,
                name,
                target: Some(owner),
                module,
                imported_name: Some(method),
                type_name: visibility,
                kind: "php_trait_alias",
                span: span(node),
            }))?;
        } else if let Some(preferred) = module {
            let mut cursor = node.walk();
            for excluded in node
                .named_children(&mut cursor)
                .skip(1)
                .filter(|n| matches!(n.kind(), "name" | "qualified_name" | "relative_name"))
            {
                let excluded = self.reference_text(excluded)?;
                (self.emit)(Fact::Binding(Binding {
                    scope: c.scope,
                    name: method.clone(),
                    target: Some(owner),
                    module: Some(excluded),
                    imported_name: Some(preferred.clone()),
                    type_name: None,
                    kind: "php_trait_exclusion",
                    span: span(node),
                }))?;
            }
        }
        Ok(())
    }
}
