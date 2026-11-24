// Purpose: lexical binding과 실제 import로 Hono 앱의 생성·별칭·basePath를 확인.
// Pattern: Provenance Resolver.
// Usage: HTTP 등록 준비가 parser의 App 값을 전달.
// Related: crates/kodstar-core/src/application/analysis/resolve/http/mod.rs, crates/kodstar-core/src/domain/web/mod.rs.

use super::super::{FileContext, Lookup};
use crate::domain::{
    diagnostics::Result,
    web::{self, App},
};
use rusqlite::{OptionalExtension, params};

pub(super) struct Instance {
    pub root: String,
    pub prefix: String,
}

impl Lookup<'_, '_> {
    pub(super) fn hono_app(
        &mut self,
        context: &FileContext,
        scope: u32,
        position: usize,
        app: &App,
        depth: usize,
    ) -> Result<Option<Instance>> {
        if depth >= 32 {
            return Ok(None);
        }
        if app.constructor {
            let (base, member) = app
                .name
                .split_once('.')
                .map_or((app.name.as_str(), None), |(a, b)| (a, Some(b)));
            let Some(binding) = self.binding(context, scope, position, base)? else {
                return Ok(None);
            };
            let module = binding.module.as_deref().unwrap_or("");
            let valid = matches!(binding.kind.as_str(), "import" | "commonjs_import")
                && matches!(module, "hono" | "hono/quick" | "hono/tiny")
                && match member {
                    Some("Hono") => binding.imported.is_none(),
                    None => binding.imported.as_deref() == Some("Hono"),
                    _ => false,
                };
            let valid = valid
                && matches!(
                    self.module_files(context, binding.scope, module, depth)?,
                    crate::application::analysis::resolve::modules::Location::External(_)
                        | crate::application::analysis::resolve::modules::Location::Unmapped
                );
            return Ok(valid.then(|| Instance {
                root: format!("{}:{scope}:{position}", context.id),
                prefix: app.prefix.clone(),
            }));
        }
        let Some(binding) = self.binding(context, scope, position, &app.name)? else {
            return Ok(None);
        };
        if binding.position > position {
            return Ok(None);
        }
        let written: bool = self.connection.cached_row("SELECT EXISTS(SELECT 1 FROM bindings WHERE file_id=?1 AND name=?2 AND kind='hono_write' AND start_byte>?3 AND start_byte<?4)",params![context.id,format!("@hono-write:{}",app.name),binding.position,position],|r|r.get(0))?;
        if written {
            return Ok(None);
        }
        if matches!(binding.kind.as_str(), "import" | "commonjs_import")
            && let Some(module) = &binding.module
        {
            let Some(mut instance) = self.hono_import(
                context,
                binding.scope,
                module,
                binding.imported.as_deref().unwrap_or("default"),
                depth + 1,
            )?
            else {
                return Ok(None);
            };
            if !app.prefix.is_empty() {
                let Some(prefix) = web::join(&instance.prefix, &app.prefix) else {
                    return Ok(None);
                };
                instance.prefix = prefix;
            }
            return Ok(Some(instance));
        }
        let data: Option<String> = self.connection.cached_row("SELECT module_name FROM bindings WHERE file_id=?1 AND scope_id=?2 AND name=?3 AND kind='hono_app' AND start_byte=?4 LIMIT 1",params![context.id,binding.scope,format!("@hono-app:{}",app.name),binding.position],|r|r.get(0)).optional()?;
        if let Some(data) = data {
            let previous: App = serde_json::from_str(&data).map_err(|e| {
                crate::domain::diagnostics::Error::caused_by("invalid Hono app evidence", e)
            })?;
            if let Some(mut instance) = self.hono_app(
                context,
                binding.scope,
                binding.position,
                &previous,
                depth + 1,
            )? {
                instance.prefix = if app.prefix.is_empty() {
                    instance.prefix
                } else {
                    match web::join(&instance.prefix, &app.prefix) {
                        Some(p) => p,
                        None => return Ok(None),
                    }
                };
                return Ok(Some(instance));
            }
        }
        if binding.kind == "alias"
            && let Some(alias) = binding.imported
        {
            return self.hono_app(
                context,
                binding.scope,
                binding.position,
                &App {
                    name: alias,
                    constructor: false,
                    prefix: app.prefix.clone(),
                },
                depth + 1,
            );
        }
        Ok(None)
    }
}
