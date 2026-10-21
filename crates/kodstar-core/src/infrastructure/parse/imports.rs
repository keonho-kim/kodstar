// Purpose: 언어별 import 구문에서 바인딩과 사용 사실 추출.
// Pattern: Simple Module.
// Usage: 참조 추출기가 import 노드에서 호출.
// Related: crates/kodstar-core/src/infrastructure/parse/references.rs.

mod qualified;

use crate::domain::diagnostics::{Error, ErrorKind, Result};
use crate::domain::facts::{Binding, Fact, SyntaxUse};
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::domain::source::SourceSpan;
use crate::infrastructure::parse::{Context, span};
use tree_sitter::Node;

pub(super) struct Imports<'a> {
    pub(super) source: &'a [u8],
    pub(super) original_source: Option<&'a [u8]>,
    pub(super) dynamic_module: Option<SourceSpan>,
    pub(super) scope: u32,
    pub(super) owner: u32,
    pub(super) condition: Option<&'a str>,
    pub(super) conditional_export: bool,
    pub(super) export: bool,
    pub(super) type_only: bool,
    pub(super) binding_kind: Option<&'static str>,
    pub(super) limit: usize,
    pub(super) emit: &'a mut dyn FnMut(Fact) -> Result<()>,
}

impl Imports<'_> {
    pub(super) fn module_origin(&mut self, node: Node<'_>) {
        self.dynamic_module = self
            .original_source
            .filter(|source| {
                crate::infrastructure::parse::validation::contains_rendering(
                    source,
                    node.byte_range(),
                )
            })
            .map(|_| span(node));
    }
    pub(super) fn bind(
        &mut self,
        local: &str,
        module: &str,
        imported: Option<&str>,
        location: SourceSpan,
    ) -> Result<()> {
        let bytes = std::mem::size_of::<Fact>()
            .saturating_add(local.len())
            .saturating_add(module.len())
            .saturating_add(imported.map_or(0, str::len));
        if bytes > self.limit {
            return Err(Error::new(
                ErrorKind::Limit,
                "import binding exceeds fact_batch_bytes",
            ));
        }
        if !local.is_empty() && local != "_" {
            (self.emit)(Fact::Binding(Binding {
                scope: self.scope,
                name: local.into(),
                target: None,
                module: self.dynamic_module.is_none().then(|| module.into()),
                imported_name: imported
                    .filter(|_| self.dynamic_module.is_none())
                    .map(str::to_owned),
                type_name: self
                    .conditional_export
                    .then(|| crate::domain::facts::CONDITIONAL_EXPORT.into()),
                kind: if self.dynamic_module.is_some() {
                    "local"
                } else {
                    self.binding_kind
                        .unwrap_or(match (self.export, self.type_only) {
                            (true, true) => "export_type",
                            (true, false) => "export",
                            (false, true) => "import_type",
                            (false, false) => "import",
                        })
                },
                span: location,
            }))?;
        }
        if let Some(position) = self.dynamic_module {
            return (self.emit)(Fact::Use(SyntaxUse {
                source: self.owner,
                scope: self.scope,
                expression: "<dynamic>()".into(),
                relation: if self.export {
                    RelationKind::Reexports
                } else {
                    RelationKind::Imports
                },
                span: position,
                condition: self.condition.map(str::to_owned),
                type_only: self.type_only,
            }));
        }
        let length = module
            .len()
            .saturating_add(imported.map_or(0, |name| name.len() + 1))
            .saturating_add(8);
        if length.saturating_add(std::mem::size_of::<Fact>()) > self.limit {
            return Err(Error::new(
                ErrorKind::Limit,
                "import reference exceeds fact_batch_bytes",
            ));
        }
        let mut expression = String::new();
        expression.try_reserve_exact(length).map_err(|error| {
            let mut error = Error::caused_by("allocate import reference", error);
            error.kind = ErrorKind::Limit;
            error
        })?;
        expression.push_str(if self.binding_kind == Some("php_import_type") {
            "@php-import-type:"
        } else if self.binding_kind == Some("php_import_function") {
            "@php-import-function:"
        } else if self.binding_kind == Some("php_import_value") {
            "@php-import-value:"
        } else if self.binding_kind == Some("commonjs_import") {
            "@require:"
        } else {
            "@module:"
        });
        expression.push_str(module);
        if let Some(name) = imported {
            expression.push('#');
            expression.push_str(name);
        }
        (self.emit)(Fact::Use(SyntaxUse {
            source: self.owner,
            scope: self.scope,
            expression,
            relation: if self.binding_kind == Some("include") {
                RelationKind::Includes
            } else if self.export {
                RelationKind::Reexports
            } else {
                RelationKind::Imports
            },
            span: location,
            condition: self.condition.map(str::to_owned),
            type_only: self.type_only,
        }))
    }
}

pub(crate) fn unquote(text: &str) -> &str {
    text.trim().trim_matches(['\'', '"', '`', '<', '>'])
}

pub(crate) fn extract(
    language: Language,
    node: Node<'_>,
    source: &[u8],
    original_source: Option<&[u8]>,
    context: &Context,
    emit: &mut dyn FnMut(Fact) -> Result<()>,
    limit: usize,
) -> Result<()> {
    let mut imports = Imports {
        source,
        original_source,
        dynamic_module: None,
        scope: context.scope,
        owner: context.source,
        condition: context.condition.as_deref(),
        conditional_export: false,
        export: false,
        type_only: false,
        binding_kind: None,
        limit,
        emit,
    };
    match language {
        Language::Rust => imports.rust(node),
        Language::Python => imports.python(node),
        Language::JavaScript | Language::TypeScript => imports.javascript(node),
        Language::Go => imports.go(node),
        Language::Ruby => imports.ruby(node),
        Language::Zig => imports.zig(node),
        Language::Swift => imports.swift(node),
        Language::Java | Language::CSharp => imports.qualified(node, '.'),
        Language::Kotlin => imports.kotlin(node),
        Language::Scala => imports.scala(node),
        Language::Php => imports.php(node),
        Language::C | Language::Cpp => imports.cxx(node),
        Language::Elixir => imports.elixir(node),
        Language::Erlang => imports.erlang(node),
        Language::Pascal => imports.pascal(node),
        _ => Ok(()),
    }
}
