// Purpose: 실제 module 파일과 명시적 export를 따라 Hono 앱 import를 해석.
// Pattern: Module Adapter.
// Usage: Hono 앱 provenance가 import binding을 만났을 때 호출.
// Related: crates/kodstar-core/src/application/analysis/resolve/http/origin.rs, crates/kodstar-core/src/application/analysis/resolve/modules/files.rs.

use super::{
    super::{FileContext, Lookup, context},
    origin::Instance,
};
use crate::domain::{
    diagnostics::{Error, Result},
    web::App,
};
use rusqlite::{OptionalExtension, params};

impl Lookup<'_, '_> {
    pub(super) fn hono_import(
        &mut self,
        caller: &FileContext,
        scope: u32,
        module: &str,
        exported: &str,
        depth: usize,
    ) -> Result<Option<Instance>> {
        if depth >= 32 {
            return Ok(None);
        }
        self.module_files(caller, scope, module, depth)?;
        let file: Option<i64> = self.connection.cached_row(
            "SELECT CASE WHEN COUNT(*)=1 THEN MIN(id) END FROM resolution_modules WHERE depth=?1",
            [depth],
            |r| r.get(0),
        )?;
        let Some(file) = file else {
            return Ok(None);
        };
        let Some(context) = context(self.connection, file, None)? else {
            return Ok(None);
        };
        let export:Option<(u32,usize,Option<String>,Option<String>)>=self.connection.cached_row("SELECT scope_id,start_byte,module_name,imported_name FROM bindings WHERE file_id=?1 AND name=?2 AND kind='export' ORDER BY id DESC LIMIT 1",params![file,exported],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        if let Some((scope, position, module, Some(name))) = export {
            return if let Some(module) = module {
                self.hono_import(&context, scope, &module, &name, depth + 1)
            } else {
                self.hono_app(
                    &context,
                    scope,
                    position,
                    &App {
                        name,
                        constructor: false,
                        prefix: String::new(),
                    },
                    depth + 1,
                )
            };
        }
        let export:Option<(u32,usize,String)>=self.connection.cached_row("SELECT scope_id,start_byte,module_name FROM bindings WHERE file_id=?1 AND name=?2 AND kind='hono_export' ORDER BY id DESC LIMIT 1",params![file,format!("@hono-export:{exported}")],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        if let Some((scope, position, data)) = export {
            let app: App = serde_json::from_str(&data)
                .map_err(|e| Error::caused_by("invalid Hono export evidence", e))?;
            return self.hono_app(&context, scope, position, &app, depth + 1);
        }
        Ok(None)
    }
}
