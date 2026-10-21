// Purpose: Pascal 분리 구현·지역 binding·암시적 호출·property 근거 추출.
// Pattern: Simple Module.
// Usage: 선언·변수·참조 추출기가 Pascal 노드를 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/pascal/signatures.rs, crates/kodstar-core/src/application/analysis/resolve/owners.rs.

pub(super) mod imports;
pub(super) mod signatures;
pub(super) mod uses;

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::pascal::signatures::header;
use crate::infrastructure::parse::{Context, Extractor, NamePath, child_kind, span, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn pascal_definition(
        &self,
        node: Node<'_>,
    ) -> Result<Option<(String, SymbolKind)>> {
        if self.language != Language::Pascal
            || !matches!(node.kind(), "defProc" | "declProc" | "declProp")
        {
            return Ok(None);
        }
        let Some(mut name) = header(node).child_by_field_name("name") else {
            return Ok(None);
        };
        let mut member = node.parent();
        let mut declared_member = false;
        while let Some(parent) = member {
            if matches!(parent.kind(), "declClass" | "declIntf") {
                declared_member = true;
                break;
            }
            member = parent.parent();
        }
        let qualified = name.kind() == "genericDot" || declared_member;
        while name.kind() == "genericDot" {
            let Some(rhs) = name.child_by_field_name("rhs") else {
                break;
            };
            name = rhs;
        }
        let mut cursor = header(node).walk();
        let constructor = header(node)
            .named_children(&mut cursor)
            .any(|n| n.kind() == "kConstructor");
        let kind = if node.kind() == "declProp" {
            SymbolKind::Field
        } else if constructor {
            SymbolKind::Constructor
        } else if qualified {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };
        Ok(Some((self.bounded_text(text(name, self.source))?, kind)))
    }

    pub(in crate::infrastructure::parse) fn pascal_owner(
        &mut self,
        node: Node<'_>,
        c: &mut Context,
    ) -> Result<()> {
        if self.language != Language::Pascal || node.kind() != "defProc" {
            return Ok(());
        }
        let Some(owner) = header(node)
            .child_by_field_name("name")
            .filter(|n| n.kind() == "genericDot")
            .and_then(|n| n.child_by_field_name("lhs"))
        else {
            return Ok(());
        };
        let name = self.reference_text(owner)?.replace('.', "::");
        let owner = NamePath::root(&name, self.fact_limit)?;
        c.implementation_owner = Some(owner.clone());
        c.type_name = Some(owner.clone());
        c.prefix = owner;
        c.member_scope = true;
        Ok(())
    }

    pub(in crate::infrastructure::parse) fn pascal_parameters(
        &mut self,
        node: Node<'_>,
        c: &Context,
    ) -> Result<()> {
        if let Some(args) = header(node).child_by_field_name("args") {
            self.parameter_list(args, c)?;
        }
        Ok(())
    }

    pub(in crate::infrastructure::parse) fn pascal_binding(
        &mut self,
        node: Node<'_>,
        c: &Context,
    ) -> Result<bool> {
        if self.language != Language::Pascal {
            return Ok(false);
        }
        if node.kind() == "declVar" {
            let ty = node
                .child_by_field_name("type")
                .map(|n| self.type_text(n))
                .transpose()?;
            let mut cursor = node.walk();
            for name in node.children_by_field_name("name", &mut cursor) {
                let spelling = self.bounded_text(text(name, self.source))?;
                (self.emit)(Fact::Binding(Binding {
                    scope: c.scope,
                    name: spelling,
                    target: None,
                    module: None,
                    imported_name: None,
                    type_name: ty.clone(),
                    kind: "local",
                    span: span(name),
                }))?;
            }
            return Ok(true);
        }
        if node.kind() == "assignment" {
            let (Some(name), Some(value)) = (
                node.child_by_field_name("lhs")
                    .filter(|n| n.kind() == "identifier"),
                node.child_by_field_name("rhs"),
            ) else {
                return Ok(false);
            };
            let producer = (value.kind() == "exprCall")
                .then(|| self.call_text(value, self.callee(value).unwrap_or(value)))
                .transpose()?;
            let spelling = self.bounded_text(text(name, self.source))?;
            (self.emit)(Fact::Binding(Binding {
                scope: c.scope,
                name: spelling,
                target: None,
                module: None,
                imported_name: producer.clone(),
                type_name: None,
                kind: if producer.is_some() {
                    "call_result"
                } else {
                    "local"
                },
                span: span(name),
            }))?;
            return Ok(true);
        }
        Ok(false)
    }

    pub(in crate::infrastructure::parse) fn pascal_facts(
        &mut self,
        node: Node<'_>,
        c: &Context,
    ) -> Result<()> {
        if self.language != Language::Pascal {
            return Ok(());
        }
        if matches!(node.kind(), "exprDot" | "identifier")
            && node.parent().is_some_and(|p| p.kind() == "statement")
        {
            self.usage(
                c,
                self.reference_text(node)?,
                RelationKind::Calls,
                span(node),
            )?;
        }
        if node.kind() == "declProp" {
            if let (Some(owner), Some(name), Some(ty)) = (
                c.type_owner,
                node.child_by_field_name("name"),
                node.child_by_field_name("type"),
            ) {
                let spelling = self.bounded_text(text(name, self.source))?;
                let annotation = self.type_text(ty)?;
                (self.emit)(Fact::Binding(Binding {
                    scope: owner,
                    name: spelling,
                    target: Some(c.source),
                    module: None,
                    imported_name: None,
                    type_name: Some(annotation),
                    kind: "field",
                    span: span(name),
                }))?;
            }
            for field in ["getter", "setter"] {
                if let (Some(owner), Some(name)) = (&c.type_name, node.child_by_field_name(field)) {
                    self.usage(
                        c,
                        format!("{}::{}", owner.render()?, self.reference_text(name)?),
                        RelationKind::References,
                        span(name),
                    )?;
                }
            }
        }
        Ok(())
    }
}

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn pascal_module_form(
        &mut self,
        node: Node<'_>,
    ) -> Result<()> {
        let name = match (self.language, node.kind()) {
            (Language::Pascal, "unit" | "program" | "library") => child_kind(node, &["moduleName"]),
            _ => None,
        };
        if let Some(name) = name {
            let name = self.bounded_text(text(name, self.source).trim_matches(['\'', '"', ':']))?;
            (self.emit)(Fact::Module {
                name,
                declaration_only: false,
            })?;
        }
        Ok(())
    }
}
