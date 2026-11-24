// Purpose: Tree-sitter 트리를 먼저 준비하고 C/C++의 동일 입력에 Clang 근거를 보강.
// Pattern: Enrichment Adapter.
// Usage: 구문 추출 진입점이 기본 트리와 선택적 native 결과를 요청.
// Related: crates/kodstar-core/src/infrastructure/parse/extraction.rs, crates/kodstar-core/src/infrastructure/parse/input/native_session.rs, crates/kodstar-core/src/infrastructure/clang/mod.rs.

use super::ParseSession;
use crate::domain::{cancellation::Cancellation, diagnostics::Result, languages::Language};
use std::{path::Path, sync::Arc};

impl ParseSession {
    pub(super) fn prepare_hybrid(
        &mut self,
        language: Language,
        grammar: &tree_sitter::Language,
        path: &Path,
        source: &[u8],
        cancel: &Cancellation,
    ) -> Result<(
        tree_sitter::Tree,
        Option<Arc<crate::infrastructure::clang::Output>>,
    )> {
        // Syntax remains independently available even when the optional compiler cannot analyze it.
        let tree = self.parse_candidate(grammar, path, source, cancel)?;
        let native = if matches!(language, Language::C | Language::Cpp) {
            self.native
                .as_mut()
                .map(|native| native.analyze(path, source, language, cancel))
                .transpose()?
        } else {
            None
        };
        let tree = self.repair_tree(language, grammar, path, source, tree, cancel)?;
        Ok((tree, native))
    }
}
