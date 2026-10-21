// Purpose: Python·TypeScript 선언에 적용된 decorator의 소유와 참조 보존.
// Pattern: Simple Module.
// Usage: 정의 생성 직후 현재 선언 context로 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/definitions.rs, crates/kodstar-core/src/infrastructure/parse/type_uses.rs.

use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::infrastructure::parse::{Context, Extractor};
use tree_sitter::Node;

impl Extractor<'_> {
    pub(super) fn decorators(&mut self, node: Node<'_>, context: &Context) -> Result<()> {
        match self.language {
            Language::Python => self.python_decorators(node, context),
            Language::TypeScript => self.typescript_decorators(node, context),
            _ => Ok(()),
        }
    }
}
