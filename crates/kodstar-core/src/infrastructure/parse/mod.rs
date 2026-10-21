// Purpose: 파일별 AST 사실 추출 순회와 언어별 구문 문맥.
// Pattern: Parser Adapter.
// Usage: 분석 worker와 format adapter가 호출.
// Related: crates/kodstar-core/src/application/analysis/pipeline.rs, crates/kodstar-core/src/infrastructure/parse/declarations.rs.

//! Parser lifetime and iterative extraction context for one input file.

mod bindings;
mod call_expressions;
mod callable_values;
mod conditions;
mod context;
mod csharp;
mod cxx;
mod declaration_metadata;
mod declarations;
mod decorators;
mod definitions;
mod elixir;
mod erlang;
mod fields;
mod forms;
mod go;
mod imports;
mod inheritance;
mod javascript;
mod kotlin;
mod lisp;
mod members;
mod namespaces;
mod nominal;
mod ownership;
mod parameters;
mod pascal;
mod php;
mod python;
mod references;
mod return_values;
mod ruby;
mod rust;
mod scala;
mod session;
mod spelling;
mod swift;
mod syntax;
mod type_uses;
mod validation;
mod zig;

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, ErrorKind, Result};
use crate::domain::facts::{Definition, Fact, Scope};
use crate::domain::languages::Language;
use crate::domain::source::SourceSpan;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::languages;
use crate::infrastructure::languages::SyntaxConfig;
use context::{Context, NamePath};
pub(crate) use session::{ParseSession, ParseSyntax};
use std::path::Path;
pub(crate) use syntax::{child_kind, span, text};

struct Extractor<'a> {
    language: Language,
    source: &'a [u8],
    config: SyntaxConfig,
    next: u32,
    fact_limit: usize,
    emit: &'a mut dyn FnMut(Fact) -> Result<()>,
    file_namespace: Option<Context>,
    lisp_package: Option<std::sync::Arc<str>>,
    swift_manifest: bool,
    cxx_header: bool,
    ejs_server: bool,
    original_source: Option<&'a [u8]>,
}

impl ParseSession {
    pub(crate) fn extract(
        &mut self,
        language: Language,
        path: &Path,
        source: &[u8],
        cancel: &Cancellation,
        emit: &mut dyn FnMut(Fact) -> Result<()>,
    ) -> Result<()> {
        self.extract_with_syntax(
            ParseSyntax {
                language,
                jsx: false,
                ejs_server: false,
                original_source: None,
            },
            path,
            source,
            cancel,
            emit,
        )
    }

    pub(crate) fn extract_with_syntax(
        &mut self,
        syntax: ParseSyntax<'_>,
        path: &Path,
        source: &[u8],
        cancel: &Cancellation,
        emit: &mut dyn FnMut(Fact) -> Result<()>,
    ) -> Result<()> {
        let ParseSyntax {
            language,
            jsx,
            ejs_server,
            original_source,
        } = syntax;
        let grammar = if language == Language::TypeScript && jsx {
            tree_sitter_typescript::LANGUAGE_TSX.into()
        } else {
            languages::grammar(language, path)?
        };
        let masked = if language == Language::Pascal {
            crate::infrastructure::parse::pascal::uses::prepare(source, cancel)?
        } else {
            std::borrow::Cow::Borrowed(source)
        };
        let tree = self.parse_tree(&grammar, path, &masked, cancel)?;
        if let Some(original) = original_source {
            if original.len() != source.len() {
                return Err(Error::operation("original and masked source ranges differ"));
            }
            validation::validate_rendered_names(&tree, original, path, cancel)?;
        }
        file_root(path, source, emit)?;
        let mut extractor = Extractor {
            language,
            source,
            config: SyntaxConfig::for_language(language),
            next: 0,
            file_namespace: None,
            lisp_package: None,
            swift_manifest: language == Language::Swift
                && path.file_name().is_some_and(|n| n == "Package.swift"),
            cxx_header: matches!(language, Language::C | Language::Cpp)
                && path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
                    ["h", "hh", "hpp", "hxx"]
                        .iter()
                        .any(|e| s.eq_ignore_ascii_case(e))
                }),
            ejs_server,
            original_source,
            fact_limit: self.fact_limit,
            emit,
        };
        let initial = Context::default();
        let mut contexts = vec![initial];
        let mut cursor = tree.root_node().walk();
        let mut steps = 0usize;
        loop {
            if contexts.len() > 1024 {
                return Err(Error::new(
                    ErrorKind::Limit,
                    "syntax nesting exceeds 1024 levels",
                ));
            }
            if steps.is_multiple_of(128) {
                cancel.check()?;
            }
            steps += 1;
            let parent = contexts
                .last()
                .ok_or_else(|| Error::operation("missing extraction context"))?
                .clone();
            let current = extractor.enter(cursor.node(), parent)?;
            contexts.push(current);
            if cursor.goto_first_child() {
                continue;
            }
            loop {
                contexts.pop();
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    return Ok(());
                }
            }
        }
    }
}

pub(crate) fn file_root(
    path: &Path,
    source: &[u8],
    emit: &mut dyn FnMut(Fact) -> Result<()>,
) -> Result<()> {
    let whole = SourceSpan::at(source, 0, source.len());
    emit(Fact::Scope(Scope {
        local_id: 0,
        parent: None,
        span: whole,
    }))?;
    emit(Fact::Definition(Definition {
        local_id: 0,
        owner: None,
        scope: 0,
        name: path.to_string_lossy().into_owned(),
        qualified_name: path.to_string_lossy().into_owned(),
        kind: SymbolKind::File,
        signature: String::new(),
        condition: None,
        span: whole,
        exported: true,
    }))
}
