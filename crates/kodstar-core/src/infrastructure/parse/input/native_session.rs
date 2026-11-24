// Purpose: 같은 parser 자원에서 C/C++·Erlang의 구문 보정과 재파싱 수명 소유.
// Pattern: Parser Session Extension.
// Usage: hybrid adapter가 준비한 기본 트리의 제한된 보정을 요청.
// Related: crates/kodstar-core/src/infrastructure/parse/hybrid.rs, crates/kodstar-core/src/infrastructure/parse/session.rs, crates/kodstar-core/src/infrastructure/parse/input/native.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::infrastructure::parse::ParseSession;
use std::path::Path;

impl ParseSession {
    pub(in crate::infrastructure::parse) fn repair_tree(
        &mut self,
        language: Language,
        grammar: &tree_sitter::Language,
        path: &Path,
        source: &[u8],
        mut tree: tree_sitter::Tree,
        cancel: &Cancellation,
    ) -> Result<tree_sitter::Tree> {
        if language == Language::Erlang
            && tree.root_node().has_error()
            && let Some((prepared, lines)) =
                super::erlang::prepare(&tree, source, self.fact_limit, cancel)?
        {
            tree = self.parse_candidate(grammar, path, &prepared, cancel)?;
            self.fragments = lines;
        }
        if language == Language::Erlang {
            super::erlang::fragments(&tree, &mut self.fragments, self.fact_limit, cancel)?;
        }
        if matches!(language, Language::C | Language::Cpp) {
            let baseline = tree.clone();
            return match self.repair_native(language, grammar, path, source, tree, cancel) {
                Err(error) if error.kind == crate::domain::diagnostics::ErrorKind::Limit => {
                    self.record_partial(&baseline);
                    Ok(baseline)
                }
                result => result,
            };
        }
        Ok(tree)
    }

    fn repair_native(
        &mut self,
        language: Language,
        grammar: &tree_sitter::Language,
        path: &Path,
        source: &[u8],
        mut tree: tree_sitter::Tree,
        cancel: &Cancellation,
    ) -> Result<tree_sitter::Tree> {
        let mut prepared = if let Some(native) = &self.native {
            native.syntax_projection(&tree, source, cancel)?
        } else {
            std::borrow::Cow::Borrowed(source)
        };
        if matches!(prepared, std::borrow::Cow::Owned(_)) {
            tree = self.parse_candidate(grammar, path, &prepared, cancel)?;
        }
        for _ in 0..8 {
            if !tree.root_node().has_error()
                && !(language == Language::Cpp
                    && super::has_macro_functions(&tree, &prepared, cancel)?)
            {
                break;
            }
            let mut markers =
                super::native::prepare(&tree, &prepared, source, self.fact_limit / 4, cancel)?;
            markers.retain(|marker| prepared[marker.offset] != marker.byte);
            if markers.is_empty() {
                break;
            }
            for marker in markers {
                prepared.to_mut()[marker.offset] = marker.byte;
            }
            drop(tree);
            tree = self.parse_candidate(grammar, path, &prepared, cancel)?;
        }
        super::native_validation::validate_original(&tree, path, source, cancel)?;
        Ok(tree)
    }
}
