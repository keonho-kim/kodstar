// Purpose: Razor의 정적 using·inject·model·inherits·namespace·page 사실 추출.
// Pattern: Format Adapter.
// Usage: Razor 영역 순회가 실제 행 지시문과 원본 범위를 전달.
// Related: crates/kodstar-core/src/infrastructure/formats/components/razor/mod.rs, crates/kodstar-core/src/application/analysis/resolve/razor.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact, SyntaxUse};
use crate::domain::relations::RelationKind;
use crate::domain::source::SourceSpan;

fn binding(
    name: &str,
    module: Option<String>,
    imported_name: Option<String>,
    type_name: Option<String>,
    kind: &'static str,
    span: SourceSpan,
) -> Fact {
    Fact::Binding(Binding {
        scope: 0,
        name: name.into(),
        target: None,
        module,
        imported_name,
        type_name,
        kind,
        span,
    })
}
fn usage(expression: String, relation: RelationKind, span: SourceSpan) -> Fact {
    Fact::Use(SyntaxUse {
        source: 0,
        scope: 0,
        expression,
        relation,
        span,
        condition: None,
        type_only: false,
    })
}
fn valid_type(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_alphanumeric() || c.is_whitespace() || "_.:<>,[]?@".contains(c))
}
fn first_type(value: &str) -> (&str, &str) {
    let mut depth = 0usize;
    for (at, c) in value.char_indices() {
        match c {
            '<' | '[' => depth += 1,
            '>' | ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if c.is_whitespace() && depth == 0 {
            return (&value[..at], value[at..].trim());
        }
    }
    (value, "")
}
fn prefix(mut span: SourceSpan, length: usize) -> SourceSpan {
    span.end_byte = span.start_byte + length;
    span.end_line = span.line;
    span.end_column = span.column + length;
    span
}

pub(in crate::infrastructure::formats::components) fn extract(
    name: &str,
    raw: &str,
    span: SourceSpan,
    emit: &mut dyn FnMut(Fact) -> Result<()>,
) -> Result<()> {
    if name == "page" {
        if let Some(route) = raw.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            emit(usage(
                format!("@route:razor:{route}"),
                RelationKind::References,
                span,
            ))?;
        }
        return Ok(());
    }
    let value = raw
        .split_once("//")
        .map_or(raw, |(value, _)| value)
        .trim()
        .trim_end_matches(';')
        .trim();
    if name == "using" {
        let (static_type, value) = value
            .strip_prefix("static ")
            .map_or((false, value), |v| (true, v.trim()));
        let (alias, target) = value
            .split_once('=')
            .map_or((None, value), |(a, t)| (Some(a.trim()), t.trim()));
        if !valid_type(target) {
            return Ok(());
        }
        let (module, imported) = if static_type || alias.is_some() {
            target
                .rsplit_once('.')
                .map_or(("", target), |(m, n)| (m, n))
        } else {
            (target, "*")
        };
        emit(binding(
            alias.unwrap_or("*"),
            Some(module.into()),
            Some(imported.into()),
            None,
            "import",
            span,
        ))?;
        emit(usage(
            format!("@module:{module}#{imported}"),
            RelationKind::Imports,
            span,
        ))?;
    } else if name == "namespace" && valid_type(value) {
        emit(binding(
            "@razor-namespace",
            Some(value.into()),
            None,
            None,
            "razor_namespace",
            span,
        ))?;
        emit(binding(
            "*",
            Some(value.into()),
            Some("*".into()),
            None,
            "import",
            span,
        ))?;
        emit(Fact::Module {
            name: value.into(),
            declaration_only: false,
        })?;
    } else if matches!(name, "model" | "inject" | "inherits") {
        let (ty, rest) = first_type(value);
        if !valid_type(ty) {
            return Ok(());
        }
        let position = prefix(span, ty.len());
        emit(usage(
            ty.into(),
            if name == "inherits" {
                RelationKind::Inherits
            } else {
                RelationKind::References
            },
            position,
        ))?;
        let variable = if name == "model" {
            Some("Model")
        } else if name == "inject"
            && !rest.is_empty()
            && rest
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '_' | '@'))
        {
            Some(rest)
        } else {
            None
        };
        if let Some(variable) = variable {
            emit(binding(
                variable,
                None,
                None,
                Some(ty.into()),
                "razor_injection",
                position,
            ))?;
        }
    }
    Ok(())
}
