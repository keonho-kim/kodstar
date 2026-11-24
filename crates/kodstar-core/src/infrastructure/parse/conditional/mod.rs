// Purpose: 조건부 소스의 모든 도달 가능한 분기를 위치 보존 투영으로 준비.
// Pattern: Conditional Parse Plan.
// Usage: 정상 tree를 얻지 못한 native 입력을 parser가 분기별로 추출.
// Related: crates/kodstar-core/src/infrastructure/parse/mod.rs, crates/kodstar-core/src/infrastructure/parse/conditional/facts.rs.

mod choices;
mod coverage;
mod directives;
mod expressions;
mod extraction;
pub(super) mod facts;
mod spill;
mod syntax;
#[cfg(test)]
mod tests;

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, ErrorKind, Result};
use crate::domain::languages::Language;
use directives::Kind;
use expressions::{Expr, Guard, Values, and, not};
use std::ops::Range;
use std::sync::Arc;
use syntax::parse;
use tree_sitter::Node;

struct Region {
    range: Range<usize>,
    guard: Guard,
}
struct Frame {
    parent: Guard,
    previous: Guard,
    closed: bool,
    alternatives: usize,
}

pub(super) struct Plan {
    regions: Vec<Region>,
    directives: Vec<Range<usize>>,
    variables: Vec<String>,
    budget: usize,
    name_bytes: usize,
}

pub(super) struct Projection<'a> {
    pub source: Vec<u8>,
    position: usize,
    plan: &'a Plan,
    values: Values,
}

impl Plan {
    pub(super) fn new(
        language: Language,
        source: &[u8],
        budget: usize,
        cancel: &Cancellation,
    ) -> Result<Option<Self>> {
        if !matches!(
            language,
            Language::C | Language::Cpp | Language::CSharp | Language::Swift
        ) {
            return Ok(None);
        }
        let directives = directives::scan(source, budget, cancel)?;
        if directives.is_empty() {
            return Ok(None);
        }
        let mut plan = Self {
            regions: Vec::new(),
            directives: Vec::new(),
            budget,
            name_bytes: 0,
            variables: Vec::new(),
        };
        let mut stack: Vec<Frame> = Vec::new();
        let mut guard = Arc::new(Expr::Constant(true));
        let mut start = 0;
        for directive in directives {
            cancel.check()?;
            plan.regions.push(Region {
                range: start..directive.range.start,
                guard: Arc::clone(&guard),
            });
            start = directive.range.end;
            plan.directives.push(directive.range);
            match directive.kind {
                Kind::If(expression) => {
                    if stack.len() >= 128 {
                        return Err(limit("nesting"));
                    }
                    let condition = parse(&expression, 0)?;
                    stack.push(Frame {
                        parent: Arc::clone(&guard),
                        previous: Arc::clone(&condition),
                        closed: false,
                        alternatives: 1,
                    });
                    guard = and(guard, condition);
                }
                Kind::ElseIf(expression) => {
                    let frame = stack.last_mut().ok_or_else(unmatched)?;
                    frame.alternatives += 1;
                    if frame.alternatives > 128 {
                        return Err(limit("branch chain"));
                    }
                    if frame.closed {
                        return Err(unmatched());
                    }
                    let condition = parse(&expression, 0)?;
                    guard = and(
                        Arc::clone(&frame.parent),
                        and(not(Arc::clone(&frame.previous)), Arc::clone(&condition)),
                    );
                    frame.previous = Arc::new(Expr::Or(Arc::clone(&frame.previous), condition));
                }
                Kind::Else => {
                    let frame = stack.last_mut().ok_or_else(unmatched)?;
                    if frame.closed {
                        return Err(unmatched());
                    }
                    frame.closed = true;
                    guard = and(Arc::clone(&frame.parent), not(Arc::clone(&frame.previous)));
                }
                Kind::End => {
                    guard = stack.pop().ok_or_else(unmatched)?.parent;
                }
            }
        }
        if !stack.is_empty() {
            return Err(unmatched());
        }
        plan.regions.push(Region {
            range: start..source.len(),
            guard,
        });
        let mut defaults = Values::new();
        for region in &plan.regions {
            region.guard.atoms(&mut defaults);
        }
        if defaults.len() > 1024 {
            return Err(limit("condition names"));
        }
        plan.name_bytes = defaults.keys().map(|key| key.len() + 64).sum();
        if plan.name_bytes.saturating_mul(2) > budget {
            return Err(limit("condition name bytes"));
        }
        plan.variables = defaults.into_keys().collect();
        Ok(Some(plan))
    }

    pub(super) fn projections<'a>(
        &'a self,
        source: &'a [u8],
        cancel: &'a Cancellation,
    ) -> choices::Choices<'a> {
        choices::Choices::new(self, source, cancel)
    }
}

impl Projection<'_> {
    pub(super) fn condition(&self, node: Node<'_>, limit_bytes: usize) -> Result<Option<String>> {
        let end = node
            .child_by_field_name("header")
            .map(|n| n.end_byte())
            .or_else(|| node.child_by_field_name("body").map(|n| n.start_byte()))
            .unwrap_or_else(|| {
                let mut cursor = node.walk();
                node.named_children(&mut cursor)
                    .find(|n| {
                        matches!(
                            n.kind(),
                            "class_body" | "function_body" | "declaration_list" | "statement_block"
                        )
                    })
                    .map_or(node.end_byte(), |n| n.start_byte())
            });
        self.condition_in(node.start_byte()..end, limit_bytes)
    }

    pub(super) fn at(&self, offset: usize, limit_bytes: usize) -> Result<Option<String>> {
        self.condition_in(
            offset..offset.saturating_add(1).min(self.source.len()),
            limit_bytes,
        )
    }

    fn condition_in(&self, range: Range<usize>, limit_bytes: usize) -> Result<Option<String>> {
        let mut condition = Arc::new(Expr::Constant(true));
        for region in &self.plan.regions {
            let start = range.start.max(region.range.start);
            let end = range.end.min(region.range.end);
            if start < end
                && region.guard.evaluate(&self.values) == Some(true)
                && self.source[start..end]
                    .iter()
                    .any(|b| !b.is_ascii_whitespace())
            {
                condition = and(condition, Arc::clone(&region.guard));
            }
        }
        if matches!(condition.as_ref(), Expr::Constant(true)) {
            return Ok(None);
        }
        let mut text = String::new();
        condition.describe(&mut text, limit_bytes)?;
        Ok(Some(text))
    }
}

fn mask(source: &mut [u8]) {
    for byte in source {
        if !matches!(*byte, b'\n' | b'\r') {
            *byte = b' ';
        }
    }
}
fn unmatched() -> Error {
    Error::input(
        "source_syntax_error",
        "unbalanced conditional compilation directives",
        None,
    )
}
fn limit(resource: &str) -> Error {
    Error::new(
        ErrorKind::Limit,
        format!("conditional compilation plan exceeds its bounded workspace: {resource}"),
    )
}
