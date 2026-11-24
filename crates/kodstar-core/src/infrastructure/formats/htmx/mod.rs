// Purpose: htmx 정적 HTTP 요청과 같은 문서의 DOM ID 참조를 원본 위치로 추출.
// Pattern: Format Adapter.
// Usage: HTML·component 시작 태그와 JSX adapter가 속성 범위를 전달.
// Related: crates/kodstar-core/src/infrastructure/formats/markup.rs, crates/kodstar-core/src/application/analysis/resolve/http/mod.rs.

mod literal;
pub(in crate::infrastructure::formats) mod scope;
pub(in crate::infrastructure) use literal::extract as literal;

use crate::domain::{
    diagnostics::Result,
    facts::{Binding, Fact, SyntaxUse},
    languages::Language,
    relations::RelationKind,
    source::SourceSpan,
};
use crate::infrastructure::formats::markup;
use std::ops::Range;

#[derive(Clone, Copy)]
pub(crate) struct Owner {
    pub language: Language,
    pub source: u32,
    pub scope: u32,
    pub disabled: bool,
}

pub(crate) fn attributes(
    source: &[u8],
    range: Range<usize>,
    origin: SourceSpan,
    owner: Owner,
    code: Option<&[u8]>,
    emit: &mut dyn FnMut(Language, Fact) -> Result<()>,
) -> Result<()> {
    let Owner {
        language,
        source: owner,
        scope,
        disabled: inherited,
    } = owner;
    let mut disabled = inherited;
    markup::attributes(source, range.clone(), &mut |a| {
        disabled |= a.name.eq_ignore_ascii_case("hx-disable")
            || a.name.eq_ignore_ascii_case("data-hx-disable");
        Ok(())
    })?;
    markup::attributes(source, range, &mut |a| {
        let name = a.name.strip_prefix("data-").unwrap_or(a.name);
        let value = html_escape::decode_html_entities(a.value);
        let dynamic = a.expression
            || code.is_some_and(|m| {
                (a.range.clone()).any(|i| {
                    m.get(i) == Some(&1)
                        || (m.get(i) == source.get(i) && !source[i].is_ascii_whitespace())
                })
            })
            || value.contains(['{', '}', '<', '>', '\0']);
        let mut location = SourceSpan::at(
            &source[origin.start_byte..],
            a.range.start - origin.start_byte,
            a.range.end - origin.start_byte,
        );
        if location.line == 1 {
            location.column += origin.column - 1;
        }
        if location.end_line == 1 {
            location.end_column += origin.column - 1;
        }
        location.line += origin.line - 1;
        location.end_line += origin.line - 1;
        location.start_byte += origin.start_byte;
        location.end_byte += origin.start_byte;
        if name.eq_ignore_ascii_case("id") && !dynamic && !value.is_empty() {
            emit(
                language,
                Fact::Binding(Binding {
                    scope,
                    name: format!("@dom-id:{}", html_escape::decode_html_entities(a.raw)),
                    target: None,
                    module: None,
                    imported_name: None,
                    type_name: None,
                    kind: "dom_id",
                    span: location,
                }),
            )?;
        }
        if disabled {
            return Ok(());
        }
        let request = match name.to_ascii_lowercase().as_str() {
            "hx-get" => Some("GET"),
            "hx-post" => Some("POST"),
            "hx-put" => Some("PUT"),
            "hx-patch" => Some("PATCH"),
            "hx-delete" => Some("DELETE"),
            _ => None,
        };
        let expression = if let Some(method) = request {
            Some((
                RelationKind::HttpRequest,
                format!(
                    "{method} {}",
                    if dynamic {
                        "<dynamic>"
                    } else if value.is_empty() {
                        "<current-document>"
                    } else {
                        &value
                    }
                ),
            ))
        } else if name.eq_ignore_ascii_case("hx-target") {
            Some((
                RelationKind::References,
                format!("@dom-target:{}", if dynamic { "<dynamic>" } else { &value }),
            ))
        } else {
            None
        };
        if let Some((relation, expression)) = expression {
            emit(
                language,
                Fact::Use(SyntaxUse {
                    source: owner,
                    scope,
                    expression,
                    relation,
                    span: location,
                    condition: None,
                    type_only: false,
                }),
            )?;
        }
        Ok(())
    })
}
