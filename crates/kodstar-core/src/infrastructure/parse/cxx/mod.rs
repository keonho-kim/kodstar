// Purpose: C/C++ declarator의 정확한 서명과 분리 메서드 소유 근거 추출.
// Pattern: Simple Module.
// Usage: 선언·매개변수 순회가 C/C++ 노드와 문맥을 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/ownership.rs, crates/kodstar-core/src/application/analysis/resolve/cxx/declarations.rs.

pub(super) mod guards;

pub(super) mod declarations;
pub(super) mod declarators;
pub(super) mod imports;
mod macros;

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact, Scope};
use crate::domain::languages::Language;
use crate::infrastructure::parse::cxx::declarators::{declarator, leaf, qualified_name};
use crate::infrastructure::parse::{Context, Extractor, NamePath, span, text};
use tree_sitter::Node;

fn internal(node: Node<'_>, source: &[u8]) -> bool {
    let mut cursor = node.walk();
    if node
        .named_children(&mut cursor)
        .any(|n| n.kind() == "storage_class_specifier" && text(n, source) == "static")
    {
        return true;
    }
    let mut parent = node.parent();
    while let Some(current) = parent {
        if current.kind() == "namespace_definition" && current.child_by_field_name("name").is_none()
        {
            return true;
        }
        if current.kind() == "function_definition" {
            return true;
        }
        parent = current.parent();
    }
    false
}

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn cxx_owner(
        &mut self,
        node: Node<'_>,
        context: &mut Context,
    ) -> Result<()> {
        if self.language != Language::Cpp || node.kind() != "function_definition" {
            return Ok(());
        }
        let Some(name) = qualified_name(node) else {
            return Ok(());
        };
        let terminal = leaf(name);
        let raw = std::str::from_utf8(&self.source[name.start_byte()..terminal.start_byte()])
            .unwrap_or("")
            .trim_end_matches(':')
            .trim();
        if raw.is_empty() {
            return Ok(());
        }
        let owner = if raw.contains("::") {
            NamePath::root(raw, self.fact_limit)?
        } else {
            context.prefix.child(raw, self.fact_limit)?
        };
        let scope = self.allocate()?;
        (self.emit)(Fact::Scope(Scope {
            local_id: scope,
            parent: Some(context.scope),
            span: span(node),
        }))?;
        context.scope = scope;
        context.prefix = owner.clone();
        context.type_name = Some(NamePath::root(
            raw.trim_start_matches("::"),
            self.fact_limit,
        )?);
        context.implementation_owner = Some(NamePath::root(
            raw.trim_start_matches("::"),
            self.fact_limit,
        )?);
        context.member_scope = true;
        Ok(())
    }

    pub(in crate::infrastructure::parse) fn cxx_callable(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        if !matches!(self.language, Language::C | Language::Cpp) {
            return Ok(());
        }
        let Some(declarator) = declarator(node) else {
            return Ok(());
        };
        let mut parent = Some(node);
        while let Some(n) = parent {
            if n.kind() == "template_declaration" {
                return Ok(());
            }
            parent = n.parent();
        }
        let Some(parameters) = declarator.child_by_field_name("parameters") else {
            return Ok(());
        };
        let mut signature = String::from("(");
        let mut cursor = parameters.walk();
        for parameter in parameters.named_children(&mut cursor) {
            if parameter.kind() == "variadic_parameter" {
                self.append_signature(&mut signature, &["..."])?;
                continue;
            }
            let Some(ty) = parameter.child_by_field_name("type") else {
                return Ok(());
            };
            let mut qualifiers = parameter.walk();
            for q in parameter
                .named_children(&mut qualifiers)
                .filter(|n| n.kind() == "type_qualifier")
            {
                let q = text(q, self.source);
                self.append_signature(&mut signature, &[q, " "])?;
            }
            let ty = self.type_text(ty)?;
            if ty == "<structural-type>" {
                return Ok(());
            }
            self.append_signature(&mut signature, &[&ty])?;
            if let Some(d) = parameter.child_by_field_name("declarator") {
                let name = if d.kind() == "identifier" {
                    Some(d)
                } else {
                    crate::infrastructure::parse::syntax::name_node(d)
                }
                .filter(|n| n.kind() == "identifier");
                self.cxx_declarator_tokens(d, name, &mut signature)?;
            }
            self.append_signature(&mut signature, &[","])?;
        }
        self.append_signature(&mut signature, &[")"])?;
        let mut qualifiers = declarator.walk();
        for q in declarator
            .named_children(&mut qualifiers)
            .filter(|n| matches!(n.kind(), "type_qualifier" | "ref_qualifier"))
        {
            self.append_signature(&mut signature, &[text(q, self.source)])?;
        }
        if let Some(result) = node.child_by_field_name("type") {
            let result = self.type_text(result)?;
            if result == "<structural-type>" {
                return Ok(());
            }
            self.append_signature(&mut signature, &["->", &result])?;
        }
        (self.emit)(Fact::Binding(Binding {
            scope: context.scope,
            name: "@cxx-signature".into(),
            target: Some(context.source),
            module: Some(signature),
            imported_name: Some(
                if node.kind() == "function_definition" {
                    "definition"
                } else {
                    "declaration"
                }
                .into(),
            ),
            type_name: Some(
                if internal(node, self.source) {
                    "internal"
                } else {
                    "external"
                }
                .into(),
            ),
            kind: "cxx_callable",
            span: span(node),
        }))
    }

    pub(in crate::infrastructure::parse) fn cxx_type(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        if !matches!(self.language, Language::C | Language::Cpp)
            || !matches!(
                node.kind(),
                "class_specifier" | "struct_specifier" | "union_specifier" | "enum_specifier"
            )
            || node.child_by_field_name("name").is_none()
        {
            return Ok(());
        }
        let mut parent = node.parent();
        while let Some(current) = parent {
            if current.kind() == "template_declaration" {
                return Ok(());
            }
            parent = current.parent();
        }
        (self.emit)(Fact::Binding(Binding {
            scope: context.scope,
            name: "@cxx-type".into(),
            target: Some(context.source),
            module: Some(
                match node.kind() {
                    "class_specifier" | "struct_specifier" => "record",
                    "union_specifier" => "union",
                    _ => "enum",
                }
                .into(),
            ),
            imported_name: Some(
                if node.child_by_field_name("body").is_some() {
                    "definition"
                } else {
                    "declaration"
                }
                .into(),
            ),
            type_name: Some(
                if internal(node, self.source) {
                    "internal"
                } else {
                    "external"
                }
                .into(),
            ),
            kind: "cxx_type",
            span: span(node),
        }))
    }

    fn cxx_declarator_tokens(
        &self,
        root: Node<'_>,
        name: Option<Node<'_>>,
        signature: &mut String,
    ) -> Result<()> {
        let mut cursor = root.walk();
        loop {
            let node = cursor.node();
            if name.is_some_and(|n| n.id() == node.id()) || node.kind().contains("comment") {
            } else if cursor.goto_first_child() {
                continue;
            } else {
                self.append_signature(signature, &["{", text(node, self.source), "}"])?;
            }
            loop {
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    return Ok(());
                }
            }
        }
    }
}
