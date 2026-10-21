// Purpose: qualified import bindings and aliases.
// Pattern: Simple Module.
// Usage: The import dispatcher delegates this language syntax.
// Related: crates/kodstar-core/src/infrastructure/parse/imports.rs.

use crate::domain::diagnostics::Result;
use crate::infrastructure::parse::imports::Imports;
use crate::infrastructure::parse::{span, text};
use tree_sitter::Node;

impl Imports<'_> {
    pub(in crate::infrastructure::parse) fn qualified(
        &mut self,
        node: Node<'_>,
        separator: char,
    ) -> Result<()> {
        let mut raw = text(node, self.source).trim().trim_end_matches(';').trim();
        for keyword in ["import ", "using ", "use ", "static "] {
            raw = raw.strip_prefix(keyword).unwrap_or(raw).trim();
        }
        let (raw, alias) = raw
            .split_once(" as ")
            .map_or((raw, None), |(path, name)| (path, Some(name.trim())));
        let (alias, raw) = raw.split_once('=').map_or((alias, raw), |(name, path)| {
            (Some(name.trim()), path.trim())
        });
        let (module, name) = raw.rsplit_once(separator).unwrap_or((raw, "*"));
        if node.kind() == "using_directive"
            && text(node, self.source)
                .split_whitespace()
                .any(|s| s == "static")
        {
            return self.bind("*", module, Some(name), span(node));
        }
        self.bind(alias.unwrap_or(name), module, Some(name), span(node))
    }
}
