// Purpose: 중첩 이름 조각을 공유하는 파일별 추출 문맥.
// Pattern: Persistent Name Path.
// Usage: parser 순회가 문맥을 복제하고 선언·바인딩이 이름을 렌더링.
// Related: crates/kodstar-core/src/infrastructure/parse/mod.rs, crates/kodstar-core/src/infrastructure/parse/declarations.rs, crates/kodstar-core/src/infrastructure/parse/bindings.rs.

use crate::domain::diagnostics::{Error, ErrorKind, Result};
use std::sync::Arc;

#[derive(Clone, Default)]
pub(super) struct NamePath(Option<Arc<Segment>>);
struct Segment {
    parent: NamePath,
    name: Arc<str>,
    length: usize,
}

impl NamePath {
    pub(super) fn root(name: &str, limit: usize) -> Result<Self> {
        Self::default().child(name, limit)
    }
    pub(super) fn child(&self, name: &str, limit: usize) -> Result<Self> {
        if self.0.is_none() && name.is_empty() {
            return Ok(Self::default());
        }
        let length = self
            .0
            .as_ref()
            .map_or(Some(0), |segment| segment.length.checked_add(2))
            .and_then(|length| length.checked_add(name.len()))
            .filter(|&length| length <= limit)
            .ok_or_else(|| {
                Error::new(ErrorKind::Limit, "qualified name exceeds fact_batch_bytes")
            })?;
        Ok(Self(Some(Arc::new(Segment {
            parent: self.clone(),
            name: Arc::from(name),
            length,
        }))))
    }
    pub(super) fn render(&self) -> Result<String> {
        let mut parts = Vec::new();
        let mut next = self.0.as_deref();
        while let Some(segment) = next {
            parts.push(segment.name.as_ref());
            next = segment.parent.0.as_deref();
        }
        let mut text = String::new();
        text.try_reserve_exact(self.0.as_ref().map_or(0, |segment| segment.length))
            .map_err(|error| {
                let mut error = Error::caused_by("allocate qualified name", error);
                error.kind = ErrorKind::Limit;
                error
            })?;
        for (index, part) in parts.into_iter().rev().enumerate() {
            if index > 0 {
                text.push_str("::");
            }
            text.push_str(part);
        }
        Ok(text)
    }
}

#[derive(Clone, Default)]
pub(super) struct Context {
    pub python_comprehension:
        Option<crate::infrastructure::parse::python::comprehensions::Comprehension>,
    pub ruby_singleton_depth: u16,
    pub lisp_outer_scope: u32,
    pub lisp_region: crate::infrastructure::parse::lisp::Region,
    pub scope: u32,
    pub source: u32,
    pub owner: u32,
    pub function_scope: u32,
    pub prefix: NamePath,
    pub type_name: Option<NamePath>,
    pub type_owner: Option<u32>,
    pub member_scope: bool,
    pub member_outer: u32,
    pub condition: Option<Arc<str>>,
    pub declaration_name: Option<(usize, usize)>,
    pub implementation_owner: Option<NamePath>,
}
