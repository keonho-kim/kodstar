// Purpose: 정적 markup 조상에서 hx-disable이 적용되는 범위를 보존.
// Pattern: Scoped State.
// Usage: template scanner가 여는·닫는 태그 순서로 갱신.
// Related: crates/kodstar-core/src/infrastructure/formats/markup.rs, crates/kodstar-core/src/infrastructure/formats/components/references.rs.

use crate::domain::diagnostics::{Error, ErrorKind, Result};
use std::ops::Range;

#[derive(Default)]
pub(in crate::infrastructure::formats) struct Scope<'a> {
    frames: Vec<(&'a str, bool)>,
}
impl<'a> Scope<'a> {
    pub fn close(&mut self, name: &str) {
        if let Some(at) = self
            .frames
            .iter()
            .rposition(|(tag, _)| tag.eq_ignore_ascii_case(name))
        {
            self.frames.truncate(at);
        }
    }
    pub fn open(
        &mut self,
        name: &'a str,
        source: &[u8],
        attributes: Range<usize>,
        closed: bool,
    ) -> Result<bool> {
        let mut disabled = self.frames.last().is_some_and(|(_, disabled)| *disabled);
        crate::infrastructure::formats::markup::attributes(source, attributes, &mut |a| {
            disabled |= a.name.eq_ignore_ascii_case("hx-disable")
                || a.name.eq_ignore_ascii_case("data-hx-disable");
            Ok(())
        })?;
        let void = [
            "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
            "source", "track", "wbr",
        ]
        .iter()
        .any(|n| name.eq_ignore_ascii_case(n));
        if !closed && !void {
            if self.frames.len() >= 1024 {
                return Err(Error::new(
                    ErrorKind::Limit,
                    "htmx markup nesting exceeds 1024 levels",
                ));
            }
            self.frames.push((name, disabled));
        }
        Ok(disabled)
    }
}
