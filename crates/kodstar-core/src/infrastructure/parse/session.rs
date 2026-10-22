// Purpose: parser 자원·grammar ABI·취소와 입력 문맥 힌트의 수명 소유.
// Pattern: Parser Adapter.
// Usage: 분석 worker·format adapter가 session을 만들고 AST 추출에 빌림.
// Related: crates/kodstar-core/src/infrastructure/parse/mod.rs, crates/kodstar-core/src/infrastructure/parse/validation.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use crate::domain::languages::Language;
use crate::infrastructure::parse::validation;
use std::ops::ControlFlow;
use std::path::Path;
use tree_sitter::{ParseOptions, Parser};

#[cfg(test)]
mod tests;

pub(crate) struct ParseSession {
    parser: Parser,
    pub(super) fact_limit: usize,
    replay: Replay,
    tree: Option<(tree_sitter::Language, blake3::Hash, tree_sitter::Tree)>,
}

enum Replay {
    Disabled,
    Remember,
    Recorded,
    Ready,
}

pub(crate) struct ParseSyntax<'a> {
    pub language: Language,
    pub jsx: bool,
    pub ejs_server: bool,
    /// Original bytes for a mask that retains the same byte ranges.
    pub original_source: Option<&'a [u8]>,
}

impl ParseSession {
    pub(crate) fn new(fact_limit: usize) -> Self {
        Self {
            parser: Parser::new(),
            // Embedded adapters remove a short synthetic qualifier before applying the final fact budget.
            fact_limit: fact_limit.saturating_add(128),
            replay: Replay::Disabled,
            tree: None,
        }
    }

    pub(crate) fn begin_file(&mut self, replay: bool) {
        self.tree = None;
        self.replay = if replay {
            Replay::Remember
        } else {
            Replay::Disabled
        };
    }

    pub(crate) fn replay(&mut self) {
        self.replay = if self.tree.is_some() {
            Replay::Ready
        } else {
            Replay::Disabled
        };
    }

    pub(crate) fn end_file(&mut self) {
        self.tree = None;
        self.replay = Replay::Disabled;
    }

    pub(crate) fn parse_tree(
        &mut self,
        grammar: &tree_sitter::Language,
        path: &Path,
        source: &[u8],
        cancel: &Cancellation,
    ) -> Result<tree_sitter::Tree> {
        cancel.check()?;
        let digest = if matches!(self.replay, Replay::Remember | Replay::Ready) {
            Some(blake3::hash(source))
        } else {
            None
        };
        if matches!(self.replay, Replay::Ready) {
            self.replay = Replay::Disabled;
            if let Some((language, previous, tree)) = self.tree.take()
                && language == *grammar
                && Some(previous) == digest
            {
                return Ok(tree);
            }
        }
        // A second AST in the preliminary pass disables retention before parsing it.
        if matches!(self.replay, Replay::Recorded) {
            self.end_file();
        }
        std::str::from_utf8(source).map_err(|error| {
            Error::caused_by(format!("non-UTF-8 source {}", path.display()), error)
        })?;
        self.parser
            .set_language(grammar)
            .map_err(|error| Error::caused_by("incompatible grammar ABI", error))?;
        let mut progress = |_: &tree_sitter::ParseState| {
            if cancel.is_cancelled() {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        };
        let tree = self.parser.parse_with_options(
            &mut |offset, _| &source[offset.min(source.len())..],
            None,
            Some(ParseOptions::new().progress_callback(&mut progress)),
        );
        cancel.check()?;
        let tree = tree
            .ok_or_else(|| Error::operation(format!("parser did not finish {}", path.display())))?;
        validation::validate(&tree, path, cancel)?;
        if matches!(self.replay, Replay::Remember) {
            self.tree = Some((
                grammar.clone(),
                digest.ok_or_else(|| Error::operation("missing replay digest"))?,
                tree.clone(),
            ));
            self.replay = Replay::Recorded;
        }
        Ok(tree)
    }
}
