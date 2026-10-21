// Purpose: C# partial 타입의 명시적 병합·generic 제외 근거 추출.
// Pattern: Simple Module.
// Usage: 타입 정의를 저장한 직후 선언 추출기가 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/declarations.rs, crates/kodstar-core/src/application/analysis/resolve/identities.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::domain::languages::Language;
use crate::infrastructure::parse::{Extractor, child_kind, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn partial_type(
        &mut self,
        node: Node<'_>,
        id: u32,
        scope: u32,
    ) -> Result<()> {
        if self.language != Language::CSharp
            || !matches!(
                node.kind(),
                "class_declaration"
                    | "struct_declaration"
                    | "interface_declaration"
                    | "record_declaration"
            )
        {
            return Ok(());
        }
        let mut ancestor = Some(node);
        while let Some(current) = ancestor {
            if current.child_by_field_name("type_parameters").is_some()
                || child_kind(current, &["type_parameter_list"]).is_some()
            {
                return Ok(());
            }
            ancestor = current.parent();
        }
        let mut cursor = node.walk();
        let partial = node
            .named_children(&mut cursor)
            .any(|n| n.kind() == "modifier" && text(n, self.source) == "partial");
        if partial {
            (self.emit)(Fact::Binding(Binding {
                scope,
                name: "@partial".into(),
                target: Some(id),
                module: None,
                imported_name: Some(node.kind().into()),
                type_name: None,
                kind: "csharp_partial",
                span: span(node),
            }))?;
        }
        Ok(())
    }
}
