// Purpose: 개별 embedded client script의 사실을 독립 ID와 원본 위치로 환산.
// Pattern: Simple Module.
// Usage: embedded adapter의 parser callback에서 호출.
// Related: crates/kodstar-core/src/infrastructure/formats/components/ejs/mod.rs, crates/kodstar-core/src/infrastructure/formats/components/astro.rs, crates/kodstar-core/src/domain/facts.rs.

use crate::domain::diagnostics::{Error, ErrorKind, Result};
use crate::domain::facts::Fact;
use crate::domain::source::SourceSpan;

pub(super) fn add(value: u32, offset: u32) -> Result<u32> {
    value
        .checked_add(offset)
        .ok_or_else(|| Error::new(ErrorKind::Limit, "embedded scope identifier overflow"))
}
fn owner(value: u32, offset: u32) -> Result<u32> {
    if value == 0 {
        Ok(0)
    } else {
        add(value, offset)
    }
}
fn scope(value: u32, offset: u32, root: u32) -> Result<u32> {
    if value == 0 {
        Ok(root)
    } else {
        add(value, offset)
    }
}
fn position(span: &mut SourceSpan, origin: SourceSpan) {
    span.start_byte += origin.start_byte;
    span.end_byte += origin.start_byte;
    if span.line == 1 {
        span.column += origin.column - 1;
    }
    if span.end_line == 1 {
        span.end_column += origin.column - 1;
    }
    span.line += origin.line - 1;
    span.end_line += origin.line - 1;
}
pub(super) fn client_fact(
    fact: &mut Fact,
    offset: u32,
    root: u32,
    origin: SourceSpan,
    maximum: &mut u32,
) -> Result<bool> {
    match fact {
        Fact::Scope(value) => {
            if value.local_id == 0 {
                return Ok(false);
            }
            value.local_id = add(value.local_id, offset)?;
            *maximum = (*maximum).max(value.local_id);
            value.parent = value.parent.map(|p| scope(p, offset, root)).transpose()?;
            position(&mut value.span, origin);
        }
        Fact::Definition(value) => {
            if value.local_id == 0 {
                return Ok(false);
            }
            value.local_id = add(value.local_id, offset)?;
            // An inline client module is not the enclosing template's public module.
            value.exported = false;
            *maximum = (*maximum).max(value.local_id);
            value.scope = scope(value.scope, offset, root)?;
            value.owner = value.owner.map(|p| owner(p, offset)).transpose()?;
            position(&mut value.span, origin);
        }
        Fact::Binding(value) => {
            value.scope = scope(value.scope, offset, root)?;
            value.target = value.target.map(|p| owner(p, offset)).transpose()?;
            position(&mut value.span, origin);
        }
        Fact::Use(value) => {
            value.source = owner(value.source, offset)?;
            value.scope = scope(value.scope, offset, root)?;
            if let Some(local) = value
                .expression
                .strip_prefix("@local:")
                .and_then(|s| s.parse::<u32>().ok())
            {
                value.expression = format!("@local:{}", owner(local, offset)?);
            }
            position(&mut value.span, origin);
        }
        Fact::Module { .. } => return Ok(false),
    }
    Ok(true)
}
