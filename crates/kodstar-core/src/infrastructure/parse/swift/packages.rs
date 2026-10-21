// Purpose: Package.swift의 정적 target·경로·소스·dependency 구문 사실 추출.
// Pattern: Simple Module.
// Usage: Swift manifest의 AST 순회가 target 생성 호출을 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/declarations.rs, crates/kodstar-core/src/application/analysis/swift_projects.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::infrastructure::parse::{Extractor, child_kind, span, text};
use tree_sitter::Node;

fn argument<'a>(call: Node<'a>, label: &str, source: &[u8]) -> Option<Node<'a>> {
    let arguments = child_kind(child_kind(call, &["call_suffix"])?, &["value_arguments"])?;
    let mut cursor = arguments.walk();
    arguments.named_children(&mut cursor).find_map(|n| {
        (n.kind() == "value_argument"
            && n.child_by_field_name("name")
                .is_some_and(|n| text(n, source) == label))
        .then(|| n.child_by_field_name("value"))
        .flatten()
    })
}

fn literal<'a>(node: Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    if node.kind() != "line_string_literal" {
        return None;
    }
    let value = text(node, source).strip_prefix('"')?.strip_suffix('"')?;
    (!value.contains(['\\', '"', '\0'])).then_some(value)
}

fn target_kind<'a>(node: Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    let callee = node.named_child(0)?;
    if callee.kind() != "prefix_expression" || !text(callee, source).starts_with('.') {
        return None;
    }
    callee
        .child_by_field_name("target")
        .map(|n| text(n, source))
}

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn swift_package(&mut self, node: Node<'_>) -> Result<()> {
        if !self.swift_manifest {
            return Ok(());
        }
        if node.kind() == "directive" {
            return self.swift_package_binding(node, "@manifest", "swift_manifest_dynamic", None);
        }
        if node.kind() != "call_expression" {
            return Ok(());
        }
        let Some(kind) = target_kind(node, self.source)
            .filter(|k| matches!(*k, "target" | "executableTarget" | "testTarget"))
        else {
            return Ok(());
        };
        let Some(array) = node.parent().filter(|n| n.kind() == "array_literal") else {
            return Ok(());
        };
        let Some(targets) = array.parent().filter(|n| {
            n.kind() == "value_argument"
                && n.child_by_field_name("name")
                    .is_some_and(|n| text(n, self.source) == "targets")
        }) else {
            return Ok(());
        };
        let package = targets
            .parent()
            .and_then(|n| n.parent())
            .and_then(|n| n.parent());
        if !package.is_some_and(|n| {
            n.kind() == "call_expression"
                && n.named_child(0).is_some_and(|n| {
                    n.kind() == "simple_identifier" && text(n, self.source) == "Package"
                })
        }) {
            return Ok(());
        }
        let Some(name) = argument(node, "name", self.source)
            .and_then(|n| literal(n, self.source))
            .filter(|n| !n.is_empty())
        else {
            return Ok(());
        };
        let name = self.bounded_text(name)?;
        let path = argument(node, "path", self.source);
        let explicit_path = path.and_then(|n| literal(n, self.source));
        let path = if let Some(path) = explicit_path {
            self.bounded_text(path)?
        } else if path.is_none() || path.is_some_and(|n| text(n, self.source) == "nil") {
            format!(
                "{}/{name}",
                if kind == "testTarget" {
                    "Tests"
                } else {
                    "Sources"
                }
            )
        } else {
            self.swift_package_binding(node, &name, "swift_target_dynamic", None)?;
            return Ok(());
        };
        self.swift_package_binding(node, &name, "swift_target", Some(&path))?;
        for (label, kind) in [
            ("sources", "swift_target_source"),
            ("exclude", "swift_target_exclude"),
        ] {
            if let Some(values) = argument(node, label, self.source) {
                if text(values, self.source) == "nil" && label == "sources" {
                    continue;
                }
                if label == "sources" {
                    self.swift_package_binding(values, &name, "swift_target_sources", None)?;
                }
                if values.kind() != "array_literal" {
                    self.swift_package_binding(values, &name, "swift_target_dynamic", None)?;
                    continue;
                }
                let mut cursor = values.walk();
                for value in values.named_children(&mut cursor).filter(|n| !n.is_extra()) {
                    if let Some(path) = literal(value, self.source) {
                        self.swift_package_binding(value, &name, kind, Some(path))?;
                    } else {
                        self.swift_package_binding(value, &name, "swift_target_dynamic", None)?;
                    }
                }
            }
        }
        if let Some(dependencies) =
            argument(node, "dependencies", self.source).filter(|n| n.kind() == "array_literal")
        {
            let mut cursor = dependencies.walk();
            for dependency in dependencies
                .named_children(&mut cursor)
                .filter(|n| !n.is_extra())
            {
                let simple = literal(dependency, self.source);
                let kind = target_kind(dependency, self.source);
                let local = matches!(kind, Some("target" | "byName"));
                let name_value = simple.or_else(|| {
                    local
                        .then(|| {
                            argument(dependency, "name", self.source)
                                .and_then(|n| literal(n, self.source))
                        })
                        .flatten()
                });
                if let Some(value) = name_value {
                    let conditional = argument(dependency, "condition", self.source)
                        .is_some_and(|n| text(n, self.source) != "nil");
                    self.swift_package_binding(
                        dependency,
                        &name,
                        if conditional {
                            "swift_target_dependency_conditional"
                        } else {
                            "swift_target_dependency"
                        },
                        Some(value),
                    )?;
                }
            }
        }
        Ok(())
    }

    fn swift_package_binding(
        &mut self,
        node: Node<'_>,
        name: &str,
        kind: &'static str,
        value: Option<&str>,
    ) -> Result<()> {
        let name = self.bounded_text(name)?;
        let value = value.map(|s| self.bounded_text(s)).transpose()?;
        (self.emit)(Fact::Binding(Binding {
            scope: 0,
            name,
            target: None,
            module: value,
            imported_name: None,
            type_name: None,
            kind,
            span: span(node),
        }))
    }
}
