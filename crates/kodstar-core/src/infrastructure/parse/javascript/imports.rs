// Purpose: ESM import·명시적 export·type-only 사실 추출.
// Pattern: Simple Module.
// Usage: import dispatcher가 JS/TS 노드를 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/imports.rs, crates/kodstar-core/src/application/analysis/resolve/modules/exports.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact, SyntaxUse};
use crate::infrastructure::parse::imports::{Imports, unquote};
use crate::infrastructure::parse::{child_kind, span, text};
use tree_sitter::Node;

impl Imports<'_> {
    pub(in crate::infrastructure::parse) fn javascript(&mut self, node: Node<'_>) -> Result<()> {
        if crate::infrastructure::parse::javascript::imports::dynamic_import(node) {
            let literal = node
                .child_by_field_name("arguments")
                .and_then(|n| n.named_child(0))
                .filter(|n| {
                    n.kind() == "string"
                        || n.kind() == "template_string" && !text(*n, self.source).contains("${")
                });
            if let Some(literal) = literal.filter(|n| !text(*n, self.source).contains('\\')) {
                self.module_origin(literal);
                return self.bind("", unquote(text(literal, self.source)), None, span(literal));
            }
            return (self.emit)(Fact::Use(SyntaxUse {
                source: self.owner,
                scope: self.scope,
                expression: "<dynamic>()".into(),
                relation: crate::domain::relations::RelationKind::Imports,
                span: span(node),
                condition: self.condition.map(str::to_owned),
                type_only: false,
            }));
        }

        if matches!(node.kind(), "variable_declarator" | "assignment_expression") {
            return self.commonjs(node);
        }
        self.export = node.kind() == "export_statement";
        self.type_only = keyword(node, "type");
        let source = node.child_by_field_name("source");
        if let Some(source) = source {
            self.module_origin(source);
        }
        let module = source.map(|n| unquote(text(n, self.source)));
        if let Some(clause) = child_kind(node, &["import_clause"]) {
            let mut cursor = clause.walk();
            for child in clause.named_children(&mut cursor) {
                match child.kind() {
                    "identifier" => self.bind(
                        text(child, self.source),
                        module.unwrap_or(""),
                        Some("default"),
                        span(child),
                    )?,
                    "namespace_import" => {
                        if let Some(name) = child_kind(child, &["identifier"]) {
                            self.bind(
                                text(name, self.source),
                                module.unwrap_or(""),
                                None,
                                span(name),
                            )?;
                        }
                    }
                    "named_imports" => self.specifiers(child, module)?,
                    _ => {}
                }
            }
        } else if let Some(clause) = child_kind(node, &["import_require_clause"]) {
            if let (Some(name), Some(source)) =
                (clause.named_child(0), clause.child_by_field_name("source"))
            {
                self.module_origin(source);
                self.bind(
                    text(name, self.source),
                    unquote(text(source, self.source)),
                    None,
                    span(name),
                )?;
            }
        } else if let Some(clause) = child_kind(node, &["export_clause"]) {
            self.specifiers(clause, module)?;
        } else if let Some(clause) = child_kind(node, &["namespace_export"]) {
            if let Some(name) = clause.named_child(0) {
                self.bind(
                    text(name, self.source),
                    module.unwrap_or(""),
                    None,
                    span(name),
                )?;
            }
        } else if let Some(module) = module {
            self.bind("*", module, Some("*"), span(node))?;
        } else if self.export && keyword(node, "default") {
            let value = node
                .child_by_field_name("declaration")
                .or_else(|| node.child_by_field_name("value"));
            if let Some(value) = value {
                let name = value.child_by_field_name("name").unwrap_or(value);
                if matches!(name.kind(), "identifier" | "type_identifier") {
                    self.local_export("default", text(name, self.source), name)?;
                } else if matches!(value.kind(), "function_expression" | "class") {
                    self.local_export(
                        "default",
                        &format!("<anonymous@{}>", value.start_byte()),
                        value,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn specifiers(&mut self, node: Node<'_>, module: Option<&str>) -> Result<()> {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            let Some(name) = child.child_by_field_name("name") else {
                continue;
            };
            let alias = child.child_by_field_name("alias").unwrap_or(name);
            let original = self.type_only;
            self.type_only |= keyword(child, "type");
            if let Some(module) = module {
                self.bind(
                    unquote(text(alias, self.source)),
                    module,
                    Some(unquote(text(name, self.source))),
                    span(child),
                )?;
            } else {
                self.local_export(
                    unquote(text(alias, self.source)),
                    unquote(text(name, self.source)),
                    child,
                )?;
            }
            self.type_only = original;
        }
        Ok(())
    }

    pub(in crate::infrastructure::parse) fn local_export(
        &mut self,
        exported: &str,
        local: &str,
        node: Node<'_>,
    ) -> Result<()> {
        let name = self.checked_name(exported)?;
        let imported_name = Some(self.checked_name(local)?);
        (self.emit)(Fact::Binding(Binding {
            scope: self.scope,
            name,
            target: None,
            module: None,
            imported_name,
            type_name: self
                .conditional_export
                .then(|| crate::domain::facts::CONDITIONAL_EXPORT.into()),
            kind: self.binding_kind.unwrap_or(if self.type_only {
                "export_type"
            } else {
                "export"
            }),
            span: span(node),
        }))
    }

    fn checked_name(&self, name: &str) -> Result<String> {
        if name
            .len()
            .saturating_mul(2)
            .saturating_add(std::mem::size_of::<Fact>())
            > self.limit
        {
            return Err(crate::domain::diagnostics::Error::new(
                crate::domain::diagnostics::ErrorKind::Limit,
                "export binding exceeds fact_batch_bytes",
            ));
        }
        Ok(name.into())
    }
}

pub(in crate::infrastructure::parse) fn keyword(node: Node<'_>, name: &str) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| !child.is_named() && child.kind() == name)
}

pub(crate) fn exported(node: Node<'_>) -> bool {
    let mut current = node;
    for _ in 0..4 {
        if current.kind() == "export_statement" {
            return !keyword(current, "default");
        }
        if matches!(current.kind(), "statement_block" | "class_body" | "program") {
            break;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent;
    }
    false
}

pub(in crate::infrastructure::parse) fn dynamic_import(node: Node<'_>) -> bool {
    node.kind() == "call_expression"
        && node
            .child_by_field_name("function")
            .is_some_and(|n| n.kind() == "import")
}
