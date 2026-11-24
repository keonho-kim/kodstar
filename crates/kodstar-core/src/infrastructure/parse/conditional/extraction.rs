// Purpose: 완전한 조건부 투영을 우선 선택하고 남은 영역만 명시적 부분 복구로 병합.
// Pattern: Bounded Two-Pass Extraction.
// Usage: parser가 오류 트리를 가진 조건부 파일의 기본 사실을 요청.
// Related: crates/kodstar-core/src/infrastructure/parse/conditional/choices.rs, crates/kodstar-core/src/infrastructure/parse/conditional/coverage.rs, crates/kodstar-core/src/infrastructure/parse/walk.rs.

use super::{Plan, facts::Merger};
use crate::{
    domain::{cancellation::Cancellation, diagnostics::Result, facts::Fact},
    infrastructure::parse::{ParseSession, file_root, input, walk},
};
use tree_sitter::Language;

impl ParseSession {
    pub(in crate::infrastructure::parse) fn extract_conditional(
        &mut self,
        input: walk::Input<'_>,
        grammar: &Language,
        plan: &Plan,
        cancel: &Cancellation,
        emit: &mut dyn FnMut(Fact) -> Result<()>,
    ) -> Result<()> {
        self.reset_syntax();
        let mut coverage = plan.coverage(input.source, cancel)?;
        let mut selected = Vec::new();
        let mut failed = Vec::new();
        let mut choices = plan.projections(input.source, cancel);
        while let Some(projection) = choices.next_projection(&coverage, !selected.is_empty())? {
            cancel.check()?;
            let masked = input::syntax(input.syntax.language, &projection.source, cancel)?;
            let tree = self.parse_candidate(grammar, input.path, &masked, cancel)?;
            let tree = self.repair_tree(
                input.syntax.language,
                grammar,
                input.path,
                &masked,
                tree,
                cancel,
            )?;
            if tree.root_node().has_error() {
                failed.push(projection.position);
            } else {
                coverage.record(&projection);
                selected.push(projection.position);
            }
        }
        drop(choices);
        // Failed combinations are irrelevant when clean views already cover every reachable region.
        if !coverage.complete() {
            for position in failed {
                let projection = plan.projection(input.source, position, cancel)?;
                if coverage.needs(plan, &projection.values) {
                    coverage.record(&projection);
                    selected.push(position);
                }
                if coverage.complete() {
                    break;
                }
            }
        }
        if let Some(range) = coverage.first_uncovered(plan) {
            self.partial = Some(crate::domain::source::SourceSpan::at(
                input.source,
                range.start,
                range.end,
            ));
        }
        file_root(input.path, input.source, emit)?;
        let mut reducer = Merger::new(self.fact_limit.saturating_mul(4), cancel, emit);
        for position in selected {
            cancel.check()?;
            let projection = plan.projection(input.source, position, cancel)?;
            let masked = input::syntax(input.syntax.language, &projection.source, cancel)?;
            let tree = self.parse_candidate(grammar, input.path, &masked, cancel)?;
            let tree = self.repair_tree(
                input.syntax.language,
                grammar,
                input.path,
                &masked,
                tree,
                cancel,
            )?;
            self.record_partial(&tree);
            reducer.begin()?;
            walk::Input {
                syntax: input.syntax,
                path: input.path,
                source: &projection.source,
                projection: Some(&projection),
            }
            .extract(&tree, self.fact_limit, cancel, &mut |fact| {
                reducer.push(fact)
            })?;
        }
        reducer.finish()
    }
}
