// Purpose: 반환 annotation의 공통 저장 계약과 Java·C# 결과 binding 추출.
// Pattern: Simple Module.
// Usage: 함수 선언과 변수 추출기가 현재 AST·문맥을 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/definitions.rs, crates/kodstar-core/src/application/analysis/resolve/returns/mod.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::domain::languages::Language;
use crate::infrastructure::parse::{Context, Extractor, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(super) fn return_annotation(
        &mut self,
        c: &Context,
        ty: Node<'_>,
        annotation: String,
    ) -> Result<()> {
        (self.emit)(Fact::Binding(Binding {
            scope: c.scope,
            name: "@return".into(),
            target: Some(c.source),
            module: None,
            imported_name: None,
            type_name: Some(annotation),
            kind: "return_type",
            span: span(ty),
        }))
    }

    pub(super) fn native_return(&mut self, node: Node<'_>, c: &Context) -> Result<()> {
        let ty = match self.language {
            Language::Java => node.child_by_field_name("type"),
            Language::CSharp => node.child_by_field_name("returns"),
            _ => None,
        };
        if let Some(ty) = ty {
            self.return_annotation(c, ty, self.type_text(ty)?)?;
        }
        Ok(())
    }

    pub(super) fn native_result_binding(&mut self, node: Node<'_>, c: &Context) -> Result<bool> {
        if !matches!(self.language, Language::Java | Language::CSharp)
            || node.kind() != "variable_declarator"
            || crate::infrastructure::parse::fields::field_variable(node)
        {
            return Ok(false);
        }
        let Some(name) = node.child_by_field_name("name") else {
            return Ok(false);
        };
        let mut cursor = node.walk();
        let value = node.child_by_field_name("value").or_else(|| {
            node.named_children(&mut cursor)
                .find(|n| n.start_byte() > name.end_byte())
        });
        let annotation = node
            .parent()
            .and_then(|p| p.child_by_field_name("type"))
            .filter(|n| !matches!(n.kind(), "implicit_type") && text(*n, self.source) != "var")
            .map(|n| self.type_text(n))
            .transpose()?;
        let producer = value
            .filter(|v| self.config.calls.contains(&v.kind()))
            .map(|v| self.call_text(v, self.callee(v).unwrap_or(v)))
            .transpose()?;
        let spelling = self.bounded_text(text(name, self.source))?;
        (self.emit)(Fact::Binding(Binding {
            scope: c.scope,
            name: spelling,
            target: None,
            module: None,
            imported_name: producer.clone(),
            type_name: annotation,
            kind: if producer.is_some() {
                "call_result"
            } else {
                "local"
            },
            span: span(name),
        }))?;
        Ok(true)
    }
}
