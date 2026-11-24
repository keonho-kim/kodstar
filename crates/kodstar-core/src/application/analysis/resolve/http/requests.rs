// Purpose: HTTP 요청·등록·응답·DOM ID 사실을 근거가 확인된 대상으로 연결.
// Pattern: Resolution Adapter.
// Usage: Lookup의 입력 dispatcher가 웹 사실을 전달.
// Related: crates/kodstar-core/src/application/analysis/resolve/http/routes.rs, crates/kodstar-core/src/domain/web/mod.rs.

use super::super::{FileContext, Lookup, inputs::Disposition};
use crate::domain::{
    diagnostics::{Error, Result},
    web::{self, Route},
};
use rusqlite::{OptionalExtension, params};

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn http_input(
        &mut self,
        context: &FileContext,
        expression: &str,
    ) -> Result<Option<Disposition>> {
        if self.relation == "http_request"
            || self.relation == "renders"
            || expression.starts_with("@dom-target:")
        {
            let response = self.hono_response_origin(context)?;
            if response == Some(false) || (self.relation == "renders" && response.is_none()) {
                return Ok(Some(Disposition::Discarded));
            }
        }
        if self.relation == "http_request" {
            self.rule = "static_http_route_candidate";
            self.candidate_only = true;
            self.reason = Some(
                "URL matches a project route; deployed origin and runtime middleware are not established",
            );
            let Some((method, url)) = expression.split_once(' ') else {
                return Ok(Some(Disposition::Handled));
            };
            let Some(path) = web::request_path(url) else {
                self.reason = Some(
                    "HTTP URL is dynamic, external or requires an established document base URL",
                );
                return Ok(Some(Disposition::Handled));
            };
            self.connection.cached_execute("INSERT OR IGNORE INTO resolution_candidates SELECT target FROM http_effective WHERE method IN (?1,'ALL','USE') AND path=?2",params![method,path])?;
            let mut q=self.connection.cached_prepare("SELECT DISTINCT path,target FROM http_effective WHERE method IN (?1,'ALL','USE') AND (instr(path,':')>0 OR instr(path,'*')>0)")?;
            let mut rows = q.query([method])?;
            while let Some(row) = rows.next()? {
                let pattern: String = row.get(0)?;
                if web::route_matches(&pattern, &path) {
                    self.connection.cached_execute(
                        "INSERT OR IGNORE INTO resolution_candidates VALUES(?1)",
                        [row.get::<_, i64>(1)?],
                    )?;
                }
            }
            let found: bool = self.connection.cached_row(
                "SELECT EXISTS(SELECT 1 FROM resolution_candidates)",
                [],
                |r| r.get(0),
            )?;
            self.dependency("http_route", &format!("{method} {path}"), found)?;
            return Ok(Some(Disposition::Handled));
        }
        if self.relation == "handles" {
            let data:Option<(u32,usize,String)>=self.connection.cached_row("SELECT b.scope_id,b.start_byte,b.module_name FROM bindings b JOIN facts f ON f.file_id=b.file_id AND f.start_byte=b.start_byte WHERE f.id=?1 AND b.kind='hono_route' LIMIT 1",[self.fact],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
            let Some((scope, position, data)) = data else {
                return Ok(Some(Disposition::Discarded));
            };
            let route: Route = serde_json::from_str(&data)
                .map_err(|e| Error::caused_by("invalid Hono route evidence", e))?;
            if self
                .hono_app(context, scope, position, &route.app, 0)?
                .is_none()
            {
                return Ok(Some(Disposition::Discarded));
            }
            self.rule = if route.method == "USE" || !route.last {
                "explicit_hono_middleware_registration"
            } else {
                "explicit_hono_registration"
            };
            self.reason = Some("Hono handler or static route expression is not established");
            self.connection.cached_execute("INSERT OR IGNORE INTO resolution_candidates SELECT target FROM http_routes WHERE file=?1 AND slot=?2",params![context.id,position])?;
            return Ok(Some(Disposition::Handled));
        }
        if self.relation == "renders" {
            let (scope, position): (u32, usize) = self.connection.cached_row(
                "SELECT scope_id,start_byte FROM facts WHERE id=?1",
                [self.fact],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            self.expression(context, scope, position, expression, 0)?;
            self.rule = "explicit_hono_component_response";
            return Ok(Some(Disposition::Handled));
        }
        if let Some(selector) = expression.strip_prefix("@dom-target:") {
            self.rule = "explicit_htmx_dom_target";
            self.reason = Some("DOM target requires a unique static #id in the same document");
            let Some(id) = selector.strip_prefix('#').filter(|s| {
                !s.is_empty()
                    && s.chars()
                        .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-'))
            }) else {
                return Ok(Some(Disposition::Handled));
            };
            let count: usize = self.connection.cached_row(
                "SELECT COUNT(*) FROM bindings WHERE file_id=?1 AND kind='dom_id' AND name=?2",
                params![context.id, format!("@dom-id:{id}")],
                |r| r.get(0),
            )?;
            if count == 1 {
                self.connection.cached_execute("INSERT OR IGNORE INTO resolution_candidates SELECT id FROM symbols WHERE file_id=?1 AND kind='file'",[context.id])?;
            }
            return Ok(Some(Disposition::Handled));
        }
        Ok(None)
    }
}

impl Lookup<'_, '_> {
    fn hono_response_origin(&mut self, context: &FileContext) -> Result<Option<bool>> {
        let value:Option<(u32,usize,String)>=self.connection.cached_row("SELECT b.scope_id,b.start_byte,b.module_name FROM bindings b JOIN facts f ON f.file_id=b.file_id AND f.start_byte=b.start_byte WHERE f.id=?1 AND b.kind='hono_response' LIMIT 1",[self.fact],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        let Some((scope, position, receiver)) = value else {
            return Ok(None);
        };
        let registered: bool = self.connection.cached_row(
            "SELECT EXISTS(SELECT 1 FROM http_routes WHERE target=?1)",
            [self.source],
            |r| r.get(0),
        )?;
        if !registered {
            return Ok(Some(false));
        }
        let parameter:Option<u32>=self.connection.cached_row("SELECT b.target_local FROM bindings b JOIN symbols s ON s.file_id=b.file_id AND s.local_id=b.target_local WHERE s.id=?1 AND b.kind='hono_context' AND b.module_name=?2 LIMIT 1",params![self.source,receiver],|r|r.get(0)).optional()?;
        Ok(Some(
            self.binding(context, scope, position, &receiver)?
                .is_some_and(|b| b.kind == "parameter" && Some(b.scope) == parameter),
        ))
    }
}
