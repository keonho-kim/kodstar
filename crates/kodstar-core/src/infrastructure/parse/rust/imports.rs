// Purpose: Rust use-tree를 한 자식씩 순회하며 공유 prefix와 별칭 보존.
// Pattern: Iterative Tree Traversal.
// Usage: import 추출기의 Rust 경로가 현재 use_declaration을 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/imports.rs, crates/kodstar-core/src/infrastructure/parse/context.rs.

use crate::domain::diagnostics::{Error, ErrorKind, Result};
use crate::infrastructure::parse::imports::Imports;
use crate::infrastructure::parse::{NamePath, span, text};
use tree_sitter::{Node, TreeCursor};

enum Frame<'tree> {
    Visit(Node<'tree>, NamePath),
    Children(TreeCursor<'tree>, NamePath, bool),
}

impl Imports<'_> {
    pub(in crate::infrastructure::parse) fn rust(&mut self, node: Node<'_>) -> Result<()> {
        self.export = text(node, self.source).trim_start().starts_with("pub");
        let root = node.child_by_field_name("argument").unwrap_or(node);
        let mut work = vec![Frame::Visit(root, NamePath::default())];
        while let Some(frame) = work.pop() {
            if work.len() >= 1024 {
                return Err(Error::new(
                    ErrorKind::Limit,
                    "use-tree nesting exceeds 1024 levels",
                ));
            }
            match frame {
                Frame::Children(mut cursor, prefix, started) => {
                    let mut present = if started {
                        cursor.goto_next_sibling()
                    } else {
                        cursor.goto_first_child()
                    };
                    while present && !cursor.node().is_named() {
                        present = cursor.goto_next_sibling();
                    }
                    if present {
                        let child = cursor.node();
                        work.push(Frame::Children(cursor, prefix.clone(), true));
                        work.push(Frame::Visit(child, prefix));
                    }
                }
                Frame::Visit(node, prefix) => match node.kind() {
                    "scoped_use_list" => {
                        let path = node
                            .child_by_field_name("path")
                            .map(|node| text(node, self.source))
                            .unwrap_or("");
                        let prefix = prefix.child(path, self.limit)?;
                        if let Some(list) = node.child_by_field_name("list") {
                            work.push(Frame::Visit(list, prefix));
                        }
                    }
                    "use_list" => work.push(Frame::Children(node.walk(), prefix, false)),
                    _ => {
                        let path = if node.kind() == "use_as_clause" {
                            node.child_by_field_name("path").unwrap_or(node)
                        } else {
                            node
                        };
                        let path = prefix
                            .child(text(path, self.source), self.limit)?
                            .render()?;
                        let path = path.trim_end_matches("::*");
                        if node.kind() == "use_wildcard" {
                            self.bind("*", path, Some("*"), span(node))?;
                            continue;
                        }
                        let alias = node
                            .child_by_field_name("alias")
                            .map(|node| text(node, self.source));
                        let (module, name) = path.rsplit_once("::").unwrap_or(("", path));
                        if name == "self" {
                            self.bind(
                                alias.unwrap_or_else(|| {
                                    module.rsplit("::").next().unwrap_or(module)
                                }),
                                module,
                                None,
                                span(node),
                            )?;
                        } else {
                            self.bind(alias.unwrap_or(name), module, Some(name), span(node))?;
                        }
                    }
                },
            }
        }
        Ok(())
    }
}
