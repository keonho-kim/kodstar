// Purpose: 정적 CommonJS require와 export 별칭 추출.
// Pattern: Simple Module.
// Usage: JS/TS import 추출기가 변수와 대입 노드를 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/javascript/imports.rs, crates/kodstar-core/src/infrastructure/parse/bindings.rs.

use crate::domain::diagnostics::Result;
use crate::infrastructure::parse::imports::{Imports, unquote};
use crate::infrastructure::parse::{span, text};
use tree_sitter::Node;

impl Imports<'_> {
    pub(in crate::infrastructure::parse) fn commonjs(&mut self, node: Node<'_>) -> Result<()> {
        if node.kind() == "variable_declarator" {
            self.binding_kind = Some("commonjs_import");
            let (Some(name), Some(value)) = (
                node.child_by_field_name("name"),
                node.child_by_field_name("value"),
            ) else {
                return Ok(());
            };
            let Some(module) = require_module(value, self.source) else {
                return Ok(());
            };
            self.module_origin(value);
            if name.kind() == "identifier" {
                self.bind(text(name, self.source), module, None, span(name))?;
            } else if name.kind() == "object_pattern" {
                let mut cursor = name.walk();
                for child in name.named_children(&mut cursor) {
                    if child.kind() == "shorthand_property_identifier_pattern" {
                        let name = text(child, self.source);
                        self.bind(name, module, Some(name), span(child))?;
                    } else if let (Some(key), Some(value)) = (
                        child.child_by_field_name("key"),
                        child.child_by_field_name("value"),
                    ) && value.kind() == "identifier"
                    {
                        self.bind(
                            text(value, self.source),
                            module,
                            Some(unquote(text(key, self.source))),
                            span(child),
                        )?;
                    }
                }
            }
        } else {
            if self.owner != 0 {
                return Ok(());
            }
            let (Some(left), Some(right)) = (
                node.child_by_field_name("left"),
                node.child_by_field_name("right"),
            ) else {
                return Ok(());
            };
            let mut parent = node.parent();
            while let Some(current) = parent {
                if matches!(
                    current.kind(),
                    "if_statement"
                        | "for_statement"
                        | "for_in_statement"
                        | "while_statement"
                        | "do_statement"
                        | "switch_statement"
                        | "try_statement"
                ) {
                    self.conditional_export = true;
                }
                parent = current.parent();
            }
            let raw = text(left, self.source);
            self.binding_kind = Some(if raw.starts_with("module.") {
                "commonjs_export_module"
            } else {
                "commonjs_export_exports"
            });
            let exported = if raw == "module.exports" {
                Some("default")
            } else {
                raw.strip_prefix("module.exports.")
                    .or_else(|| raw.strip_prefix("exports."))
            };
            if let Some(exported) = exported.filter(|n| !n.is_empty() && !n.contains('.')) {
                self.export = true;
                if right.kind() == "identifier" {
                    self.local_export(exported, text(right, self.source), right)?;
                } else if matches!(
                    right.kind(),
                    "arrow_function" | "function_expression" | "class"
                ) {
                    self.local_export(exported, raw, right)?;
                } else if raw == "module.exports" && right.kind() == "object" {
                    let mut cursor = right.walk();
                    for pair in right.named_children(&mut cursor) {
                        if pair.kind() == "shorthand_property_identifier" {
                            self.local_export(
                                text(pair, self.source),
                                text(pair, self.source),
                                pair,
                            )?;
                        } else if let (Some(key), Some(value)) = (
                            pair.child_by_field_name("key"),
                            pair.child_by_field_name("value"),
                        ) && value.kind() == "identifier"
                            && matches!(key.kind(), "property_identifier" | "string")
                        {
                            self.local_export(
                                unquote(text(key, self.source)),
                                text(value, self.source),
                                pair,
                            )?;
                        }
                    }
                } else if let Some(module) = require_module(right, self.source) {
                    self.module_origin(right);
                    self.bind(exported, module, None, span(right))?;
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn require_module<'a>(node: Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    if node.kind() != "call_expression"
        || text(node.child_by_field_name("function")?, source) != "require"
    {
        return None;
    }
    let args = node.child_by_field_name("arguments")?;
    let value = args.named_child(0)?;
    if args.named_child_count() != 1 || value.kind() != "string" {
        return None;
    }
    Some(unquote(text(value, source)))
}
