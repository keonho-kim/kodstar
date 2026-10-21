// Purpose: Pascal 매개변수 mode·타입·반환의 제한된 서명과 대응 근거 추출.
// Pattern: Simple Module.
// Usage: 선언 서명 표시와 Pascal 선언/정의 메타데이터가 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/pascal/mod.rs, crates/kodstar-core/src/application/analysis/resolve/pascal_declarations.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::infrastructure::parse::{Context, Extractor, span, text};
use tree_sitter::Node;

pub(in crate::infrastructure::parse) fn header(node: Node<'_>) -> Node<'_> {
    node.child_by_field_name("header").unwrap_or(node)
}

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn pascal_signature(
        &self,
        node: Node<'_>,
    ) -> Result<String> {
        let header = header(node);
        let mut result = String::new();
        let mut tokens = header.walk();
        let kind = header.named_children(&mut tokens).find(|n| {
            matches!(
                n.kind(),
                "kProcedure" | "kFunction" | "kConstructor" | "kDestructor"
            )
        });
        self.append_signature(
            &mut result,
            &[kind.map_or("procedure", |n| text(n, self.source)), "("],
        )?;
        if let Some(args) = header.child_by_field_name("args") {
            let mut cursor = args.walk();
            for parameter in args
                .named_children(&mut cursor)
                .filter(|n| n.kind() == "declArg")
            {
                let ty = parameter
                    .child_by_field_name("type")
                    .map(|n| self.type_text(n))
                    .transpose()?
                    .unwrap_or_else(|| "<untyped>".into());
                let mut fields = parameter.walk();
                let mode = parameter
                    .named_children(&mut fields)
                    .find(|n| matches!(n.kind(), "kVar" | "kConst" | "kOut" | "kConstref"))
                    .map_or("value", |n| text(n, self.source));
                let mut names = parameter.walk();
                for _ in parameter.children_by_field_name("name", &mut names) {
                    self.append_signature(&mut result, &[mode, ":", &ty, ";"])?;
                }
            }
        }
        self.append_signature(&mut result, &[")"])?;
        if let Some(ty) = header.child_by_field_name("type") {
            self.append_signature(&mut result, &[":", &self.type_text(ty)?])?;
        }
        Ok(result.to_ascii_lowercase())
    }

    pub(in crate::infrastructure::parse) fn pascal_callable(
        &mut self,
        node: Node<'_>,
        c: &Context,
    ) -> Result<()> {
        let signature = self.pascal_signature(node)?;
        if signature.contains(['<', '{']) {
            return Ok(());
        }
        (self.emit)(Fact::Binding(Binding {
            scope: c.scope,
            name: "@pascal-signature".into(),
            target: Some(c.source),
            module: Some(signature),
            imported_name: Some(
                if node.kind() == "defProc" {
                    "definition"
                } else {
                    "declaration"
                }
                .into(),
            ),
            type_name: None,
            kind: "pascal_signature",
            span: span(node),
        }))?;
        if let Some(ty) = header(node).child_by_field_name("type") {
            self.return_annotation(c, ty, self.type_text(ty)?)?;
        }
        Ok(())
    }
}
