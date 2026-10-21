// Purpose: Lisp reader package 전환과 scope별 package 소유 정보 추출.
// Pattern: Simple Module.
// Usage: 선언 순회와 Lisp 선언 배치가 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/lisp/definitions.rs, crates/kodstar-core/src/infrastructure/parse/declarations.rs.

use crate::domain::diagnostics::{Error, Result};
use crate::domain::facts::{Binding, Definition, Fact, Scope, SyntaxUse};
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::domain::resolve::lisp::key;
use crate::domain::source::SourceSpan;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::parse::lisp::form_value;
use crate::infrastructure::parse::{Context, Extractor, NamePath, span, text};
use std::sync::Arc;
use tree_sitter::Node;

pub(in crate::infrastructure::parse) fn designator(
    node: Node<'_>,
    source: &[u8],
) -> Option<String> {
    match node.kind() {
        "sym_lit" => Some(key(text(node, source))),
        "kwd_lit" => form_value(node, 0).map(|n| key(text(n, source))),
        "str_lit" => {
            let raw = text(node, source).strip_prefix('"')?.strip_suffix('"')?;
            let mut result = String::new();
            let mut escaped = false;
            for c in raw.chars() {
                if escaped {
                    result.push(c);
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else {
                    result.push(c);
                }
            }
            (!escaped).then_some(result)
        }
        "quoting_lit" => form_value(node, 0).and_then(|n| designator(n, source)),
        _ => None,
    }
}

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn lisp_package_definition(
        &mut self,
        node: Node<'_>,
        id: u32,
    ) -> Result<()> {
        if self.language != Language::CommonLisp
            || node.kind() != "list_lit"
            || !form_value(node, 0).is_some_and(|n| key(text(n, self.source)) == "DEFPACKAGE")
        {
            return Ok(());
        }
        if let Some(name) = form_value(node, 1)
            && let Some(package) = designator(name, self.source)
        {
            let package = self.bounded_text(&package)?;
            (self.emit)(Fact::Binding(Binding {
                scope: 0,
                name: "@package".into(),
                target: Some(id),
                module: Some(package.clone()),
                imported_name: None,
                type_name: None,
                kind: "lisp_pkg_def",
                span: span(name),
            }))?;
            let mut cursor = node.walk();
            for option in node
                .named_children(&mut cursor)
                .filter(|n| n.kind() == "list_lit")
            {
                let Some(head) = form_value(option, 0).and_then(|n| designator(n, self.source))
                else {
                    continue;
                };
                let kind = match head.as_str() {
                    "USE" => "lisp_pkg_use",
                    "EXPORT" => "lisp_pkg_export",
                    "SHADOW" => "lisp_pkg_shadow",
                    "INTERN" => "lisp_pkg_intern",
                    "NICKNAMES" => "lisp_pkg_nickname",
                    "IMPORT-FROM" => "lisp_pkg_import",
                    "SHADOWING-IMPORT-FROM" => "lisp_pkg_shadowing_import",
                    _ => continue,
                };
                let from = matches!(head.as_str(), "IMPORT-FROM" | "SHADOWING-IMPORT-FROM");
                let target = if from {
                    form_value(option, 1).and_then(|n| designator(n, self.source))
                } else {
                    None
                };
                if from && target.is_none() {
                    continue;
                }
                let mut values = option.walk();
                for value in option
                    .named_children(&mut values)
                    .filter(|n| !matches!(n.kind(), "comment" | "block_comment"))
                    .skip(if from { 2 } else { 1 })
                {
                    let Some(value_name) = designator(value, self.source) else {
                        continue;
                    };
                    let value_name = self.bounded_text(&value_name)?;
                    let target = target
                        .as_deref()
                        .map(|s| self.bounded_text(s))
                        .transpose()?;
                    (self.emit)(Fact::Binding(Binding {
                        scope: 0,
                        name: "@package-option".into(),
                        target: Some(id),
                        module: Some(package.clone()),
                        imported_name: Some(value_name),
                        type_name: target,
                        kind,
                        span: span(value),
                    }))?;
                }
            }
        }
        Ok(())
    }

    pub(in crate::infrastructure::parse) fn lisp_package_context(
        &mut self,
        node: Node<'_>,
        context: &mut Context,
    ) -> Result<()> {
        if self.language != Language::CommonLisp {
            return Ok(());
        }
        if node.parent().is_none() {
            self.lisp_package_binding(0, 0, "CL-USER", span(node))?;
        }
        if !node.parent().is_some_and(|p| p.parent().is_none()) {
            return Ok(());
        }
        let head = form_value(node, 0).filter(|n| n.kind() == "sym_lit");
        if node.kind() == "list_lit"
            && head.is_some_and(|n| key(text(n, self.source)) == "IN-PACKAGE")
        {
            let name = form_value(node, 1)
                .ok_or_else(|| Error::operation("missing Lisp package designator"))?;
            let package = designator(name, self.source)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    Error::operation("Lisp package designator is not statically established")
                })?;
            let mut end = self.source.len();
            let mut next = node.next_named_sibling();
            while let Some(sibling) = next {
                if sibling.kind() == "list_lit"
                    && form_value(sibling, 0)
                        .is_some_and(|n| key(text(n, self.source)) == "IN-PACKAGE")
                {
                    end = sibling.start_byte();
                    break;
                }
                next = sibling.next_named_sibling();
            }
            let (scope, prefix) = self.lisp_package_scope(&package, node, end)?;
            self.file_namespace = Some(Context {
                scope,
                owner: scope,
                prefix,
                ..Context::default()
            });
            self.lisp_package = Some(Arc::from(package.as_str()));
            (self.emit)(Fact::Use(SyntaxUse {
                source: 0,
                scope,
                expression: format!("@lisp-package:{package}"),
                relation: RelationKind::Imports,
                span: span(name),
                condition: None,
                type_only: false,
            }))?;
        }
        if let Some(active) = &self.file_namespace {
            *context = active.clone();
        }
        Ok(())
    }

    pub(in crate::infrastructure::parse) fn lisp_package_scope(
        &mut self,
        package: &str,
        node: Node<'_>,
        end: usize,
    ) -> Result<(u32, NamePath)> {
        let package = self.bounded_text(package)?;
        let id = self.allocate()?;
        (self.emit)(Fact::Scope(Scope {
            local_id: id,
            parent: Some(0),
            span: SourceSpan::at(self.source, node.start_byte(), end),
        }))?;
        (self.emit)(Fact::Definition(Definition {
            local_id: id,
            owner: Some(0),
            scope: 0,
            name: package.clone(),
            qualified_name: package.clone(),
            kind: SymbolKind::Module,
            signature: String::new(),
            condition: None,
            span: span(node),
            exported: true,
        }))?;
        self.lisp_package_binding(id, id, &package, span(node))?;
        Ok((id, NamePath::root(&package, self.fact_limit)?))
    }

    fn lisp_package_binding(
        &mut self,
        scope: u32,
        target: u32,
        package: &str,
        at: SourceSpan,
    ) -> Result<()> {
        let package = self.bounded_text(package)?;
        (self.emit)(Fact::Binding(Binding {
            scope,
            name: "@reader_package".into(),
            target: Some(target),
            module: Some(package),
            imported_name: None,
            type_name: None,
            kind: "lisp_scope_package",
            span: at,
        }))
    }
}
