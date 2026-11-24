// Purpose: 파일별 AST 사실 추출 순회와 언어별 구문 문맥.
// Pattern: Parser Adapter.
// Usage: 분석 worker와 format adapter가 호출.
// Related: crates/kodstar-core/src/application/analysis/pipeline.rs, crates/kodstar-core/src/infrastructure/parse/declarations.rs.

//! Parser lifetime and iterative extraction context for one input file.

mod bindings;
mod call_expressions;
mod callable_values;
mod conditional;
mod conditions;
mod context;
mod csharp;
mod cxx;
mod declaration_metadata;
mod declarations;
mod decorators;
mod definitions;
mod documentation;
mod elixir;
mod erlang;
mod extraction;
mod fields;
mod forms;
mod go;
mod hybrid;
mod imports;
mod inheritance;
mod input;
mod java;
mod javascript;
mod kotlin;
mod lisp;
mod members;
mod namespaces;
mod nominal;
mod ownership;
mod parameters;
mod php;
mod python;
mod recovery;
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
mod walk;
mod zig;

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::domain::facts::{Definition, Fact, Scope};
use crate::domain::languages::Language;
use crate::domain::source::SourceSpan;
use crate::domain::symbols::SymbolKind;
use crate::infrastructure::languages::SyntaxConfig;
use context::{Context, NamePath};
pub(crate) use session::{ParseSession, ParseSyntax};
use std::path::Path;
pub(crate) use syntax::{child_kind, span, text};

struct Extractor<'a> {
    language: Language,
    source: &'a [u8],
    cancel: &'a Cancellation,
    config: SyntaxConfig,
    next: u32,
    fact_limit: usize,
    emit: &'a mut dyn FnMut(Fact) -> Result<()>,
    file_namespace: Option<Context>,
    lisp_package: Option<std::sync::Arc<str>>,
    swift_manifest: bool,
    cxx_header: bool,
    native_macro_context: bool,
    ejs_server: bool,
    htmx: bool,
    hono_contexts: std::collections::BTreeSet<u32>,
    original_source: Option<&'a [u8]>,
    projection: Option<&'a conditional::Projection<'a>>,
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
