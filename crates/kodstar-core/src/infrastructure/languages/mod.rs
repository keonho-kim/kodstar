// Purpose: 언어 판별·공식 grammar 선택·capability 구성.
// Pattern: Simple Module.
// Usage: scan·parser·Engine 언어 보고가 사용.
// Related: crates/kodstar-core/src/infrastructure/languages/syntax.rs.

//! Canonical upstream Tree-sitter grammars. No language-specific process is executed.

mod header;
mod syntax;

use std::path::Path;

use crate::domain::diagnostics::{Error, Result};
use crate::domain::languages::Language;
use crate::domain::results::LanguageCapability;

pub(crate) use syntax::SyntaxConfig;

pub(crate) fn grammar(language: Language, path: &Path) -> Result<tree_sitter::Language> {
    Ok(match language {
        Language::Python => tree_sitter_python::LANGUAGE.into(),
        Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        Language::TypeScript => {
            if path.extension().is_some_and(|extension| extension == "tsx") {
                tree_sitter_typescript::LANGUAGE_TSX.into()
            } else {
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
            }
        }
        Language::Rust => tree_sitter_rust::LANGUAGE.into(),
        Language::Go => tree_sitter_go::LANGUAGE.into(),
        Language::Java => tree_sitter_java::LANGUAGE.into(),
        Language::C => tree_sitter_c::LANGUAGE.into(),
        Language::Cpp => tree_sitter_cpp::LANGUAGE.into(),
        Language::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
        Language::Php => tree_sitter_php::LANGUAGE_PHP.into(),
        Language::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
        Language::Swift => tree_sitter_swift::LANGUAGE.into(),
        Language::Ruby => tree_sitter_ruby::LANGUAGE.into(),
        Language::Scala => tree_sitter_scala::LANGUAGE.into(),
        Language::Zig => tree_sitter_zig::LANGUAGE.into(),
        Language::Elixir => tree_sitter_elixir::LANGUAGE.into(),
        Language::Erlang => tree_sitter_erlang::LANGUAGE.into(),
        Language::CommonLisp => tree_sitter_commonlisp::LANGUAGE_COMMONLISP.into(),
        _ => {
            return Err(Error::operation(
                "mixed input must be split into original-position embedded ranges before parsing",
            ));
        }
    })
}

pub(crate) fn detect(path: &Path, prefix: &[u8]) -> Option<Language> {
    let name = path.file_name()?.to_string_lossy();
    if matches!(name.as_ref(), "Gemfile" | "Rakefile" | "Guardfile") {
        return Some(Language::Ruby);
    }
    if name.ends_with(".blade.php") {
        return Some(Language::Blade);
    }
    if path.extension().is_some_and(|extension| extension == "h") {
        return header::detect(prefix);
    }
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        return Language::ALL
            .into_iter()
            .find(|language| language.extensions().contains(&extension));
    }
    let first = prefix.split(|&byte| byte == b'\n').next()?;
    let line = std::str::from_utf8(first).ok()?;
    if !line.starts_with("#!") {
        return None;
    }
    let words: Vec<_> = line[2..].split_ascii_whitespace().collect();
    let executable = if words.first().is_some_and(|word| word.ends_with("/env")) {
        words
            .iter()
            .skip(1)
            .find(|word| !word.starts_with('-'))
            .copied()?
    } else {
        words.first().copied()?
    };
    let executable = executable.rsplit('/').next()?;
    match executable {
        value if value == "python" || value.starts_with("python3") => Some(Language::Python),
        "node" | "nodejs" => Some(Language::JavaScript),
        "ruby" => Some(Language::Ruby),
        "php" => Some(Language::Php),
        "elixir" => Some(Language::Elixir),
        "escript" => Some(Language::Erlang),
        "sbcl" | "clisp" => Some(Language::CommonLisp),
        _ => None,
    }
}

pub(crate) fn capability(language: Language) -> LanguageCapability {
    let mut limitations = vec![
        "static syntax and explicit bindings only; dynamic targets may remain unresolved",
        "no project execution, build scripts or compiler plugins; runtime dispatch and complete type inference are not proven",
    ];
    limitations.push(match language {
        Language::Rust => "separate type/value/bang-macro lookups, raw identifiers, inline imports, tuple/Self constructors, impl owners and explicit macro_export crate-root paths/textual/reexports supported; visibility, cfg_attr-derived exports, macro_use imports, NFC equivalence, lifetimes and full trait/generic selection remain incomplete",
        Language::Python => "global/nonlocal, comprehension/outer-iterable/walrus scopes, static __all__ wildcard exports, module binding reexports, proven function-value references/indirect candidates, unshadowed static getattr, property reads, C3/super, constructors, annotated returns and nearest pytest fixtures supported; dynamic decorators, arbitrary export evaluation and complete inference remain incomplete",
        Language::JavaScript | Language::TypeScript => "static Hono apps, route/middleware registrations, imported sub-app mounts and JSX/static HTML responses supported; runtime route factories and renderer execution are not evaluated; JSX/TSX component tags and ESM/CommonJS, JSONC path configs, indexed workspace package exports/imports and proven function-value references/indirect candidates supported; conditional CommonJS exports remain candidates; dynamic modules and full type/value namespaces remain incomplete",
        Language::Go => "go.mod/go.work, indexed local replacements, exported package access, receiver/field embedding and interface name/arity candidates supported; MVS/version selection, complete signature and pointer method sets remain incomplete",
        Language::Java => "annotation class literals and indexed package qualification supported; explicit imports, typed fields, constructors/annotated return chains and interface/override candidates supported; overloads, access checks and transitive dispatch remain incomplete",
        Language::CSharp => "block/file namespaces, non-generic partial types within static SDK/Compile membership, linked sources, XAML project context and constructor/annotated return chains supported; conditional/dynamic/imported membership is diagnosed and isolated; per-project source instances, generic/partial methods, overloads, access checks and transitive dispatch remain incomplete",
        Language::C | Language::Cpp => "header namespaces, typed receivers, include-backed top-level forward types/free functions, canonical whole-header guards and exact-signature C++ member declarations supported; internal linkage stays file-local and conflicting definitions stay ambiguous; nested forward, conditional includes, compiler modules and complete type equivalence remain incomplete; statically linked Clang proves successful-TU callee selection, templates and generated function definitions in its observed context; absent SDK/headers, unsupported arguments, resource limits and unevaluated conditional declarations are diagnosed or retain syntax candidates",
        Language::Ruby => "static require_relative, Class/Struct/Data factories, constructor locals, singleton/instance sides and explicit mixin candidates supported; dynamic lookup, refinements, visibility mutations, runtime ordering and load paths remain incomplete",
        Language::Swift => "annotated parameters/returns, constructors, typed/factory properties, optional receivers and proven SwiftUI Environment wrappers supported; static SwiftPM targets/sources/excludes/dependencies and selected imports supported; generic extensions, local package products, dynamic manifests, testable imports, access and overload selection remain incomplete",
        Language::Kotlin => "escaped names, AST import aliases, annotation names and static class-literal arguments supported; declared package/FQN, object/companion owners, aliased imports, constructor locals and annotated return chains supported; extension selection, type aliases, complete generic/access and Gradle module semantics remain incomplete",
        Language::Scala => "named/anonymous given ownership, type aliases/match/context bounds, generic scope, curried parameters, constructor fields and typed/local-return receivers supported; class/object spaces and explicit apply separated; implicit search, singleton/path-dependent types, generic substitution and access/override selection remain incomplete",
        Language::Php => "block/semicolon namespaces, grouped class/function/const imports, function fallback, typed constructor/return receivers and static trait adaptation with method access checks supported; union types, property access, magic/late binding and dynamic callables remain incomplete",
        Language::Zig => "static file @import/projections, pub boundaries, named containers, tagged-union and error-set cases supported; @cImport, build-module maps, comptime evaluation, quoted identifiers and inferred values remain incomplete",
        Language::Elixir => "nested modules, protocol/defimpl containers, alias groups, arity/clauses/defaults/pipelines, explicit captures and only/except imports supported; protocol runtime dispatch, dynamic/multiple implementation target expansion, alias-derived implementation names, dynamic macros and full binding remain incomplete",
        Language::Erlang => "name/arity clauses, function/type export visibility, static type/opaque/macro declarations, included records/macros, type-spec references and explicit function values supported; complete spec-to-function ownership, preprocessing and behaviour/callback checking remain incomplete",
        Language::CommonLisp => "default ASCII reader, quote/backquote, lexical functions, standard macro/symbol/setf/compiler definers and static package/use/import/export/shadow/nickname lookup supported; compiler/setf bindings do not shadow normal calls; package mutation, CLOS applicability, evaluated custom definers and custom readtables remain incomplete",
        Language::Html => "embedded JavaScript, static htmx requests, hx-disable scopes and same-file #id target references supported; browser base URLs, runtime DOM mutation, other inherited htmx settings and arbitrary CSS selectors are not evaluated",
        Language::Vue | Language::Svelte => "explicit imported template components and implicit component defaults supported; Vue kebab-case tags and Vue/Svelte bare event handlers preserve original ranges; script/style/comments/attribute/interpolation strings are excluded; module/setup/instance boundaries and full template expression or framework semantics remain incomplete",
        Language::Astro => "TypeScript server frontmatter and template imports are isolated from browser scripts; processed TypeScript modules have independent scopes, inline classic globals are shared and client exports do not escape as component exports; original ranges and implicit component defaults are preserved; runtime define:vars, bundling, page-level script order and full template expressions are not evaluated",
        Language::Razor => "static using/alias/static imports, model/inject/inherits/namespace/page, code/functions methods, bare event handlers and project-scoped component files supported; unproven default namespaces remain candidates; statement blocks, _Imports hierarchy, generated classes, implicit routes and full Razor compilation are not modeled",
        Language::Ejs => "server tags, shadow-aware literal relative includes, require/import and sequential client script extraction preserve original ranges; server/client and sibling module scopes are separated while classic globals are shared; rendered client relations remain candidates with doctor diagnostics, generated identifiers fail atomically; runtime root/views/includer options, generated JavaScript and execution order are not evaluated",
        Language::Blade => "literal extends/include/includeIf/component view paths, comment/verbatim/escape-aware PHP blocks and inline php preserve original ranges; Livewire component references retain framework identities and wire:click action names remain unresolved without an established owner; registered/namespaced views, dynamic paths, PHP heredocs, complete echo expressions, custom directives and runtime component registration are not evaluated",
    });
    if language == Language::TypeScript {
        limitations.push("class/member/parameter decorators preserve declaration owners and references; decorator evaluation is not performed");
    }
    LanguageCapability {
        language: language.as_str(),
        input_kind: if language.is_component() {
            "component_or_template"
        } else {
            "programming_language"
        },
        extensions: language.extensions(),
        input_recognition: true,
        definition_extraction: true,
        file_relationships: true,
        direct_relationships: true,
        semantic_enrichment: matches!(
            language,
            Language::Rust
                | Language::Python
                | Language::JavaScript
                | Language::TypeScript
                | Language::Go
                | Language::Java
                | Language::C
                | Language::Cpp
                | Language::CSharp
                | Language::Php
                | Language::Kotlin
                | Language::Scala
                | Language::Swift
                | Language::Ruby
                | Language::Elixir
        ),
        provider: if matches!(language, Language::C | Language::Cpp) {
            "tree-sitter + embedded Clang"
        } else if language.is_component() {
            "tree-sitter + embedded-range scanner"
        } else {
            "tree-sitter"
        },
        limitations,
    }
}
