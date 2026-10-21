// Purpose: Pascal unit의 별칭·공개 이름 import와 명시적 파일 경로 바인딩 추출.
// Pattern: Simple Module.
// Usage: import dispatcher가 declUses를 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/imports.rs, crates/kodstar-core/src/infrastructure/parse/pascal/uses.rs.

use crate::domain::diagnostics::{Error, ErrorKind, Result};
use crate::domain::facts::{Binding, Fact, SyntaxUse};
use crate::domain::relations::RelationKind;
use crate::infrastructure::parse::imports::Imports;
use crate::infrastructure::parse::{span, text};
use tree_sitter::Node;

impl Imports<'_> {
    pub(in crate::infrastructure::parse) fn pascal(&mut self, node: Node<'_>) -> Result<()> {
        let mut cursor = node.walk();
        for unit in node
            .named_children(&mut cursor)
            .filter(|n| n.kind() == "moduleName")
        {
            let name = text(unit, self.source);
            let mut next = unit.next_named_sibling();
            while next.is_some_and(|n| n.kind() != "moduleName") {
                next = next.and_then(|n| n.next_named_sibling());
            }
            let end = next.map_or(node.end_byte(), |n| n.start_byte());
            let file = crate::infrastructure::parse::pascal::uses::file_after(
                self.source,
                unit.end_byte(),
                end,
                self.limit,
            )?;
            let module = file
                .as_ref()
                .map_or_else(|| name.to_owned(), |p| format!("@pascal-file:{p}"));
            if std::mem::size_of::<Fact>()
                .saturating_add(name.len())
                .saturating_add(module.len())
                .saturating_add(16)
                > self.limit
            {
                return Err(Error::new(
                    ErrorKind::Limit,
                    "Pascal import exceeds fact_batch_bytes",
                ));
            }
            for (local, imported) in [(name, None), ("*", Some("*"))] {
                (self.emit)(Fact::Binding(Binding {
                    scope: self.scope,
                    name: local.into(),
                    target: None,
                    module: Some(module.clone()),
                    imported_name: imported.map(str::to_owned),
                    type_name: None,
                    kind: "import",
                    span: span(unit),
                }))?;
            }
            let expression =
                file.map_or_else(|| format!("@module:{name}"), |p| format!("@file:{p}"));
            (self.emit)(Fact::Use(SyntaxUse {
                source: self.owner,
                scope: self.scope,
                expression,
                relation: RelationKind::Imports,
                span: span(unit),
                condition: None,
                type_only: false,
            }))?;
        }
        Ok(())
    }
}
