// Purpose: EJS tag를 한 개씩 읽어 server 코드와 client placeholder mask 생성.
// Pattern: Streaming Scanner.
// Usage: EJS adapter가 같은 버퍼를 server/client 순서로 재사용.
// Related: crates/kodstar-core/src/infrastructure/formats/components/ejs/mod.rs, crates/kodstar-core/src/infrastructure/formats/components/regions.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, Result};
use crate::infrastructure::formats::components::regions::{blank, find};

#[derive(Clone, Copy, PartialEq)]
pub(in crate::infrastructure::formats::components) enum Kind {
    Code,
    Output,
    Comment,
    Escape,
}
pub(in crate::infrastructure::formats::components) struct Tag {
    pub start: usize,
    pub begin: usize,
    pub body_end: usize,
    pub end: usize,
    pub kind: Kind,
}
pub(in crate::infrastructure::formats::components) struct Tags<'a> {
    source: &'a [u8],
    at: usize,
}
impl<'a> Tags<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self { source, at: 0 }
    }
    pub fn next(&mut self, cancel: &Cancellation) -> Result<Option<Tag>> {
        cancel.check()?;
        let Some(start) = find(self.source, self.at, b"<%") else {
            return Ok(None);
        };
        let marker = self.source.get(start + 2).copied();
        if marker == Some(b'%') {
            self.at = start + 3;
            return Ok(Some(Tag {
                start,
                begin: start + 3,
                body_end: start + 3,
                end: start + 3,
                kind: Kind::Escape,
            }));
        }
        let closing = find(self.source, start + 2, b"%>")
            .ok_or_else(|| Error::input("source_syntax_error", "EJS tag is incomplete", None))?;
        let kind = match marker {
            Some(b'=' | b'-') => Kind::Output,
            Some(b'#') => Kind::Comment,
            _ => Kind::Code,
        };
        let begin = start + 2 + usize::from(matches!(marker, Some(b'=' | b'-' | b'#' | b'_')));
        let body_end = closing
            - usize::from(closing > begin && matches!(self.source[closing - 1], b'-' | b'_'));
        self.at = closing + 2;
        Ok(Some(Tag {
            start,
            begin,
            body_end,
            end: closing + 2,
            kind,
        }))
    }
}

pub(in crate::infrastructure::formats::components) fn server(
    source: &[u8],
    mask: &mut [u8],
    cancel: &Cancellation,
) -> Result<bool> {
    let mut tags = Tags::new(source);
    let mut code = false;
    while let Some(tag) = tags.next(cancel)? {
        if matches!(tag.kind, Kind::Code | Kind::Output) {
            mask[tag.begin..tag.body_end].copy_from_slice(&source[tag.begin..tag.body_end]);
            if tag.kind == Kind::Output {
                if let Some(last) = source[tag.begin..tag.body_end]
                    .iter()
                    .rposition(|b| !b.is_ascii_whitespace())
                    && source[tag.begin + last] == b';'
                {
                    mask[tag.begin + last] = b' ';
                }
                mask[tag.start] = b'(';
                mask[tag.end - 2] = b')';
                mask[tag.end - 1] = b';';
            }
            code = true;
        }
    }
    Ok(code)
}

pub(in crate::infrastructure::formats::components) fn exclude_tags(
    source: &[u8],
    mask: &mut [u8],
    cancel: &Cancellation,
) -> Result<()> {
    let mut tags = Tags::new(source);
    while let Some(tag) = tags.next(cancel)? {
        mask[tag.start..tag.end].fill(1);
    }
    Ok(())
}
pub(in crate::infrastructure::formats::components) fn neutralize_tags(
    source: &[u8],
    mask: &mut [u8],
    cancel: &Cancellation,
) -> Result<bool> {
    let mut tags = Tags::new(source);
    let mut rendered = false;
    while let Some(tag) = tags.next(cancel)? {
        let inside = mask[tag.start] == source[tag.start];
        blank(&source[tag.start..tag.end], &mut mask[tag.start..tag.end]);
        if inside {
            rendered = true;
            if tag.kind == Kind::Output {
                if let Some(last) = source[tag.begin..tag.body_end]
                    .iter()
                    .rposition(|b| !b.is_ascii_whitespace())
                    && source[tag.begin + last] == b';'
                {
                    mask[tag.begin + last] = b' ';
                }
                mask[tag.start] = b'0';
            }
        }
    }
    Ok(rendered)
}
