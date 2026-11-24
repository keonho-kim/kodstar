// Purpose: 검증된 AST의 반복 순회·중첩 문맥과 취소 경계.
// Pattern: Tree Visitor.
// Usage: 언어별 입력 준비 후 parser 진입점이 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/mod.rs, crates/kodstar-core/src/infrastructure/parse/declarations.rs.

use super::{Context, Extractor, ParseSyntax, SyntaxConfig, conditional, recovery};
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, ErrorKind, Result};
use crate::domain::facts::Fact;
use crate::domain::languages::Language;
use std::path::Path;

pub(super) struct Input<'a> {
    pub syntax: ParseSyntax<'a>,
    pub path: &'a Path,
    pub source: &'a [u8],
    pub projection: Option<&'a conditional::Projection<'a>>,
}

impl Input<'_> {
    pub(super) fn extract(
        &self,
        tree: &tree_sitter::Tree,
        limit: usize,
        cancel: &Cancellation,
        emit: &mut dyn FnMut(Fact) -> Result<()>,
    ) -> Result<()> {
        let language = self.syntax.language;
        let mut extractor = Extractor {
            language,
            source: self.source,
            cancel,
            config: SyntaxConfig::for_language(language),
            next: 0,
            file_namespace: None,
            lisp_package: None,
            swift_manifest: language == Language::Swift
                && self.path.file_name().is_some_and(|n| n == "Package.swift"),
            cxx_header: matches!(language, Language::C | Language::Cpp)
                && self
                    .path
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| {
                        ["h", "hh", "hpp", "hxx"]
                            .iter()
                            .any(|e| s.eq_ignore_ascii_case(e))
                    }),
            native_macro_context: false,
            ejs_server: self.syntax.ejs_server,
            htmx: memchr::memmem::find(self.source, b"hx-").is_some(),
            hono_contexts: std::collections::BTreeSet::new(),
            original_source: self.syntax.original_source,
            projection: self.projection,
            fact_limit: limit,
            emit,
        };
        extractor.walk(tree, cancel)
    }
}

impl Extractor<'_> {
    pub(super) fn walk(&mut self, tree: &tree_sitter::Tree, cancel: &Cancellation) -> Result<()> {
        let initial = Context::default();
        let mut contexts = vec![(None, initial, false)];
        let mut cursor = tree.root_node().walk();
        let mut steps = 0usize;
        loop {
            if contexts.len() > 1024 {
                return Err(Error::new(
                    ErrorKind::Limit,
                    "syntax nesting exceeds 1024 levels",
                ));
            }
            if steps.is_multiple_of(128) {
                cancel.check()?;
            }
            steps += 1;
            let (parent_node, parent, opaque) = contexts
                .last()
                .ok_or_else(|| Error::operation("missing extraction context"))?
                .clone();
            let node = cursor.node();
            let special = (node.has_error() || opaque) && self.special_definition(node)?.is_some();
            let action = recovery::action(node, &self.config, opaque, special);
            let (current, parent_node, opaque) = match action {
                recovery::Action::Enter | recovery::Action::HeaderOnly => (
                    self.enter(
                        node,
                        parent,
                        parent_node,
                        contexts.len() == 2 || parent_node.is_none(),
                    )?,
                    Some(node),
                    false,
                ),
                recovery::Action::Transparent => (parent, parent_node, true),
                recovery::Action::Skip => (parent, parent_node, true),
            };
            contexts.push((parent_node, current, opaque));
            if !matches!(
                action,
                recovery::Action::Skip | recovery::Action::HeaderOnly
            ) && cursor.goto_first_child()
            {
                continue;
            }
            loop {
                contexts.pop();
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
