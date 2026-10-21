// Purpose: Lisp 전역 선언 home package·본문 lexical 환경의 배치 분리.
// Pattern: Simple Module.
// Usage: 특수 선언 분류와 단일 정의 생성이 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/forms.rs, crates/kodstar-core/src/infrastructure/parse/definitions.rs.

use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::domain::resolve::Reference;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::lisp::definers::global_name;
use crate::infrastructure::parse::{Extractor, NamePath, text};
use tree_sitter::Node;

pub(in crate::infrastructure::parse) struct Placement {
    pub scope: u32,
    pub owner: u32,
    pub prefix: NamePath,
}

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn lisp_global_definition(
        &self,
        node: Node<'_>,
    ) -> Result<Option<(String, SymbolKind)>> {
        if self.language != Language::CommonLisp {
            return Ok(None);
        }
        let Some((name, kind)) = global_name(node, self.source) else {
            return Ok(None);
        };
        let raw = self.bounded_text(text(name, self.source))?;
        let name = if kind == SymbolKind::Module {
            crate::infrastructure::parse::lisp::packages::designator(name, self.source)
                .unwrap_or(raw)
        } else if let Reference::LispPackage { symbol, .. } =
            crate::domain::resolve::reference(self.language, &raw)
        {
            symbol
        } else {
            raw
        };
        Ok(Some((name, kind)))
    }

    pub(in crate::infrastructure::parse) fn lisp_placement(
        &mut self,
        node: Node<'_>,
    ) -> Result<Option<Placement>> {
        if self.language != Language::CommonLisp {
            return Ok(None);
        }
        let Some((name, kind)) = global_name(node, self.source) else {
            return Ok(None);
        };
        if kind == SymbolKind::Module {
            return Ok(Some(Placement {
                scope: 0,
                owner: 0,
                prefix: NamePath::default(),
            }));
        }
        if let Reference::LispPackage { package, .. } =
            crate::domain::resolve::reference(self.language, text(name, self.source))
            && self.lisp_package.as_deref().unwrap_or("CL-USER") != package
        {
            let (scope, prefix) = self.lisp_package_scope(&package, name, node.end_byte())?;
            return Ok(Some(Placement {
                scope,
                owner: scope,
                prefix,
            }));
        }
        let active = self.file_namespace.as_ref();
        Ok(Some(Placement {
            scope: active.map_or(0, |c| c.scope),
            owner: active.map_or(0, |c| c.owner),
            prefix: active.map_or_else(NamePath::default, |c| c.prefix.clone()),
        }))
    }
}
