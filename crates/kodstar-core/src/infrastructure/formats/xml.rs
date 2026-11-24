// Purpose: 읽기 전용 XML 문서에서 project·resource·event 관계 추출.
// Pattern: Format Adapter.
// Usage: auxiliary가 보존 XML 원문과 파일 작업 예산을 전달.
// Related: crates/kodstar-core/src/infrastructure/formats/auxiliary/mod.rs, crates/kodstar-core/src/infrastructure/formats/dotnet.rs.

mod attributes;
pub(super) mod positions;
#[cfg(test)]
mod tests;
pub(super) mod values;
use crate::domain::{
    cancellation::Cancellation,
    diagnostics::{Error, Result},
    facts::{Binding, Fact, SyntaxUse},
    relations::RelationKind,
};
use crate::infrastructure::parse::ParseSession;
use positions::Positions;
use std::path::Path;

pub(super) fn tag_name<'a>(node: roxmltree::Node<'_, '_>, source: &'a [u8]) -> &'a str {
    if !node.is_element() {
        return "";
    }
    let raw = &source[node.range().start + 1..];
    let end = raw
        .iter()
        .position(|byte| byte.is_ascii_whitespace() || matches!(byte, b'/' | b'>'))
        .unwrap_or(0);
    std::str::from_utf8(&raw[..end]).unwrap_or("")
}

fn element_text(node: roxmltree::Node<'_, '_>, name: &str, source: &[u8]) -> Option<String> {
    let child = node
        .children()
        .find(|child| tag_name(*child, source) == name)?;
    let mut value = String::new();
    for text in child.children().filter(|child| child.is_text()) {
        value.push_str(text.text()?);
    }
    Some(value.trim().to_string())
}

pub(crate) fn extract(
    parser: &mut ParseSession,
    path: &Path,
    source: &[u8],
    cancel: &Cancellation,
    emit: &mut dyn FnMut(Fact) -> Result<()>,
) -> Result<()> {
    cancel.check()?;
    let text = simdutf8::compat::from_utf8(source)
        .map_err(|error| Error::caused_by("non-UTF-8 XML source", error))?;
    let document = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            nodes_limit: u32::try_from(source.len().saturating_add(1)).unwrap_or(u32::MAX),
            entity_resolver: None,
        },
    )
    .map_err(|error| Error::caused_by(format!("invalid XML {}", path.display()), error))?;
    cancel.check()?;
    let positions = Positions::new(source, parser.fact_limit() / 4);
    let project = path.extension().is_some_and(|s| s == "csproj");
    if project {
        emit(Fact::Binding(Binding {
            scope: 0,
            name: "@project".into(),
            target: None,
            module: path.parent().and_then(Path::to_str).map(str::to_owned),
            imported_name: None,
            type_name: None,
            kind: "dotnet_project",
            span: positions.span(
                if source.starts_with(b"\xef\xbb\xbf") {
                    3
                } else {
                    0
                }..source.len(),
            ),
        }))?;
    }
    let mut attributes = attributes::Attributes::default();
    for node in document.descendants().filter(|node| node.is_element()) {
        cancel.check()?;
        if node
            .ancestors()
            .filter(|ancestor| ancestor.is_element())
            .count()
            > 1024
        {
            return Err(Error::operation("XML nesting exceeds 1024 levels"));
        }
        if project {
            super::dotnet::visit(node, source, &positions, emit)?;
        }
        if tag_name(node, source) == "dependency"
            && let (Some(group), Some(artifact)) = (
                element_text(node, "groupId", source),
                element_text(node, "artifactId", source),
            )
        {
            emit(Fact::Use(SyntaxUse {
                source: 0,
                scope: 0,
                expression: format!("@package:maven:{group}:{artifact}"),
                relation: RelationKind::DependsOn,
                span: positions.span(node.range()),
                condition: None,
                type_only: false,
            }))?;
        }
        attributes.visit(node, path, source, &positions, emit)?;
    }
    Ok(())
}
