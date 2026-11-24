// Purpose: 원문 디코딩·기본 구문 그래프와 선택적 의미 보강의 추출 진입점.
// Pattern: Parser Pipeline.
// Usage: worker와 format adapter가 같은 파일 수명의 ParseSession을 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/hybrid.rs, crates/kodstar-core/src/infrastructure/parse/recovery.rs, crates/kodstar-core/src/infrastructure/parse/conditional/mod.rs.

use super::{ParseSession, ParseSyntax, conditional, file_root, input, validation, walk};
use crate::domain::{
    cancellation::Cancellation, diagnostics::Result, facts::Fact, languages::Language,
};
use std::path::Path;
use tree_sitter::Tree;

impl ParseSession {
    pub(crate) fn extract(
        &mut self,
        language: Language,
        path: &Path,
        source: &[u8],
        cancel: &Cancellation,
        emit: &mut dyn FnMut(Fact) -> Result<()>,
    ) -> Result<()> {
        let decoded = input::encoding::Source::read(language, source, self.fact_limit, cancel)?;
        if let Some(decoded) = decoded {
            let earlier = self.partial.take();
            let result =
                self.extract_utf8(language, path, &decoded.bytes, cancel, &mut |mut fact| {
                    decoded.restore(&mut fact);
                    emit(fact)
                });
            self.remap_partial(earlier, |span| decoded.restore_span(span));
            return result;
        }
        self.extract_utf8(language, path, source, cancel, emit)
    }

    fn extract_utf8(
        &mut self,
        language: Language,
        path: &Path,
        source: &[u8],
        cancel: &Cancellation,
        emit: &mut dyn FnMut(Fact) -> Result<()>,
    ) -> Result<()> {
        self.extract_with_syntax(
            ParseSyntax {
                language,
                jsx: false,
                ejs_server: false,
                original_source: None,
            },
            path,
            source,
            cancel,
            emit,
        )
    }

    pub(crate) fn extract_with_syntax(
        &mut self,
        syntax: ParseSyntax<'_>,
        path: &Path,
        source: &[u8],
        cancel: &Cancellation,
        emit: &mut dyn FnMut(Fact) -> Result<()>,
    ) -> Result<()> {
        let grammar = if syntax.language == Language::TypeScript && syntax.jsx {
            tree_sitter_typescript::LANGUAGE_TSX.into()
        } else {
            crate::infrastructure::languages::grammar(syntax.language, path)?
        };
        let masked = input::syntax(syntax.language, source, cancel)?;
        let (tree, native) =
            self.prepare_hybrid(syntax.language, &grammar, path, &masked, cancel)?;
        if syntax.language == Language::TypeScript {
            input::typescript::validate(&tree, source, path, cancel)?;
        }
        if let Some(original) = syntax.original_source {
            if original.len() != source.len() {
                return Err(crate::domain::diagnostics::Error::operation(
                    "original and masked source ranges differ",
                ));
            }
            validation::validate_rendered_names(&tree, original, path, cancel)?;
        }
        if let Some(native) = native {
            let mut projection =
                crate::infrastructure::clang::Projection::new(native, source, emit);
            self.extract_tree(
                walk::Input {
                    syntax,
                    path,
                    source,
                    projection: None,
                },
                &grammar,
                tree,
                cancel,
                &mut |fact| projection.push(fact),
            )?;
            return projection.finish();
        }
        self.extract_tree(
            walk::Input {
                syntax,
                path,
                source,
                projection: None,
            },
            &grammar,
            tree,
            cancel,
            emit,
        )
    }

    fn extract_tree(
        &mut self,
        input: walk::Input<'_>,
        grammar: &tree_sitter::Language,
        tree: Tree,
        cancel: &Cancellation,
        emit: &mut dyn FnMut(Fact) -> Result<()>,
    ) -> Result<()> {
        if tree.root_node().has_error() {
            match conditional::Plan::new(
                input.syntax.language,
                input.source,
                self.fact_limit,
                cancel,
            ) {
                Ok(Some(plan)) => {
                    return self.extract_conditional(input, grammar, &plan, cancel, emit);
                }
                Err(error) if error.input.is_none() => return Err(error),
                _ => {}
            }
            self.record_partial(&tree);
        }
        file_root(input.path, input.source, emit)?;
        input.extract(&tree, self.fact_limit, cancel, emit)
    }
}
