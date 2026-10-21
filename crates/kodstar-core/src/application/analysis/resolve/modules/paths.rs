// Purpose: 선언 scope 기준 Rust·Python 상대 module 경로 정규화.
// Pattern: Simple Module.
// Usage: module_files가 원래 경로와 import 선언 scope를 전달.
// Related: crates/kodstar-core/src/application/analysis/resolve/modules/mod.rs, crates/kodstar-core/src/infrastructure/parse/imports.rs.

use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn module_path(
        &self,
        context: &FileContext,
        scope: u32,
        raw: &str,
    ) -> Result<String> {
        let module = if context.language == Language::Rust {
            if raw.is_empty() {
                "crate".into()
            } else if raw == "self"
                || raw.starts_with("self::")
                || raw == "super"
                || raw.starts_with("super::")
            {
                let nested = crate::application::analysis::resolve::namespaces::nearest_module(
                    self.connection,
                    context.id,
                    scope,
                )?
                .map(|m| m.1);
                let base = nested.map_or_else(
                    || context.module.clone(),
                    |n| format!("{}::{n}", context.module),
                );
                if raw == "self" {
                    base
                } else if let Some(tail) = raw.strip_prefix("self::") {
                    format!("{base}::{tail}")
                } else {
                    let mut parts: Vec<_> = base.split("::").collect();
                    let mut tail = raw;
                    loop {
                        if tail == "super" {
                            parts.pop();
                            tail = "";
                            break;
                        }
                        if let Some(next) = tail.strip_prefix("super::") {
                            parts.pop();
                            tail = next;
                        } else {
                            break;
                        }
                    }
                    if parts.is_empty() {
                        "<outside-crate>".into()
                    } else if tail.is_empty() {
                        parts.join("::")
                    } else {
                        format!("{}::{tail}", parts.join("::"))
                    }
                }
            } else if raw == "crate" || raw.starts_with("crate::") {
                raw.into()
            } else {
                format!("crate::{raw}")
            }
        } else if context.language == Language::Elixir
            && (raw == "__MODULE__" || raw.starts_with("__MODULE__."))
        {
            let module = crate::application::analysis::resolve::namespaces::nearest_module(
                self.connection,
                context.id,
                scope,
            )?
            .map(|m| m.1);
            let base = module.unwrap_or_default().replace("::", ".");
            format!("{base}{}", raw.strip_prefix("__MODULE__").unwrap_or(""))
        } else if context.language == Language::Python && raw.starts_with('.') {
            let dots = raw.bytes().take_while(|&b| b == b'.').count();
            let mut base: Vec<_> = context.module.split('.').collect();
            if context.path.file_stem().is_none_or(|n| n != "__init__") {
                base.pop();
            }
            for _ in 1..dots {
                base.pop();
            }
            if raw[dots..].is_empty() {
                base.join(".")
            } else if base.is_empty() {
                raw[dots..].into()
            } else {
                format!("{}.{}", base.join("."), &raw[dots..])
            }
        } else {
            raw.replace("::", ".")
        };
        Ok(
            if matches!(context.language, Language::Rust | Language::Kotlin) {
                context.language.lookup_key(&module)
            } else {
                module
            },
        )
    }
}
