// Purpose: Go build tag와 C 계열 전처리 분기의 선언 조건 보존.
// Pattern: Simple Module.
// Usage: 선언 메타데이터가 언어별 조건을 요청.
// Related: crates/kodstar-core/src/infrastructure/parse/declaration_metadata.rs, crates/kodstar-core/src/infrastructure/parse/declarations.rs.

use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::infrastructure::parse::{Extractor, text};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(super) fn native_condition(&self, node: Node<'_>) -> Result<Option<String>> {
        if let Some(projection) = self.projection {
            return projection.condition(node, self.fact_limit);
        }
        if self.language == Language::Go {
            for line in std::str::from_utf8(self.source).unwrap_or("").lines() {
                let line = line.trim();
                if let Some(tag) = line.strip_prefix("//go:build ") {
                    return self.bounded_text(tag).map(Some);
                }
                if !line.is_empty() && !line.starts_with("//") {
                    break;
                }
            }
        }
        if !matches!(
            self.language,
            Language::C | Language::Cpp | Language::CSharp
        ) {
            return Ok(None);
        }
        let mut current = Some(node);
        let mut condition = String::new();
        let mut alternative = false;
        while let Some(node) = current {
            if matches!(node.kind(), "preproc_if" | "preproc_ifdef" | "preproc_elif") {
                let value = node
                    .child_by_field_name("condition")
                    .or_else(|| node.child_by_field_name("name"));
                if let Some(value) = value {
                    let raw = text(value, self.source);
                    let directive = if text(node, self.source).trim_start().starts_with("#ifndef") {
                        "!defined"
                    } else if node.kind() == "preproc_ifdef" {
                        "defined"
                    } else {
                        ""
                    };
                    let empty = condition.is_empty();
                    self.append_signature(
                        &mut condition,
                        &[
                            if empty { "" } else { " && " },
                            if alternative { "!(" } else { "(" },
                            directive,
                            "(",
                            raw,
                            "))",
                        ],
                    )?;
                }
                alternative = node.kind() == "preproc_elif";
            } else if node.kind() == "preproc_else" {
                alternative = true;
            }
            current = node.parent();
        }
        Ok((!condition.is_empty()).then_some(condition))
    }
}
