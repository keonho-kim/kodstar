// Purpose: native 선언 근거가 있는 enum 매크로·함수 매개변수 wrapper만 원문 위치로 투영.
// Pattern: Source Adapter.
// Usage: parser가 구조 매크로를 포함한 tree를 재분석하기 전에 호출.
// Related: crates/kodstar-core/src/infrastructure/clang/mod.rs, crates/kodstar-core/src/infrastructure/parse/input/native_session.rs.

use super::Session;
use crate::domain::{cancellation::Cancellation, diagnostics::Result};
use std::{borrow::Cow, collections::BTreeSet};
use tree_sitter::Tree;

impl Session {
    pub(crate) fn syntax_projection<'a>(
        &self,
        tree: &Tree,
        source: &'a [u8],
        cancel: &Cancellation,
    ) -> Result<Cow<'a, [u8]>> {
        let mut result = Cow::Borrowed(source);
        let Some(output) = self.replay.as_ref().filter(|r| {
            matches!(
                r.analysis.status,
                kodstar_clang::Status::Complete | kodstar_clang::Status::Incomplete
            )
        }) else {
            return Ok(result);
        };
        let mut macros = BTreeSet::new();
        let mut enums = std::collections::BTreeMap::new();
        for fact in output.analysis.facts.iter() {
            cancel.check()?;
            if fact.kind == 3 && fact.defined {
                enums.insert(fact.begin, fact.end);
            }
            if matches!(fact.kind, 4 | 5)
                && fact.generated
                && fact.begin < fact.end
                && fact.end <= source.len()
            {
                macros.insert((fact.begin, fact.end));
            }
            if fact.kind == 1
                && let Some((begin, end)) = fact.parameters
                && fact.name_end < begin
                && begin < end
                && end <= source.len()
                && source[begin] == b'('
                && source[end - 1] == b')'
            {
                let prefix = source[fact.name_end..begin].trim_ascii();
                if let Some(name) = prefix.strip_suffix(b"(").map(<[u8]>::trim_ascii)
                    && !name.is_empty()
                    && name.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_')
                {
                    let mut close = end;
                    while source.get(close).is_some_and(u8::is_ascii_whitespace) {
                        close += 1;
                    }
                    if source.get(close) == Some(&b')') {
                        erase(result.to_mut(), fact.name_end, begin);
                        erase(result.to_mut(), close, close + 1);
                    }
                }
            }
        }
        if macros.is_empty() {
            return Ok(result);
        }
        let mut cursor = tree.root_node().walk();
        loop {
            cancel.check()?;
            let node = cursor.node();
            if node.kind() == "enum_specifier"
                && let Some(body) = node.child_by_field_name("body")
            {
                let boundary = enums
                    .get(&node.start_byte())
                    .copied()
                    .unwrap_or(body.end_byte());
                for &(begin, mut end) in
                    macros.range((body.start_byte(), 0)..=(boundary, usize::MAX))
                {
                    if end > boundary {
                        continue;
                    }
                    let mut comma = end;
                    while source.get(comma).is_some_and(u8::is_ascii_whitespace) {
                        comma += 1;
                    }
                    if source.get(comma) == Some(&b',') {
                        end = comma + 1;
                    }
                    erase(result.to_mut(), begin, end);
                }
            }
            if cursor.goto_first_child() {
                continue;
            }
            loop {
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    return Ok(result);
                }
            }
        }
    }
}

fn erase(source: &mut [u8], begin: usize, end: usize) {
    for byte in &mut source[begin..end] {
        if !matches!(byte, b'\r' | b'\n') {
            *byte = b' ';
        }
    }
}
