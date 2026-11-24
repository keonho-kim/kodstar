// Purpose: HTTP 등록과 앱 표현의 parser·resolver 간 구조화된 계약.
// Pattern: Value Objects.
// Usage: Hono 추출기와 HTTP resolver가 같은 직렬화 계약을 사용.
// Related: crates/kodstar-core/src/domain/web/paths.rs.

mod paths;
pub(crate) use paths::{join, request_path, route_matches};

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub(crate) struct App {
    pub name: String,
    pub constructor: bool,
    pub prefix: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub(crate) struct Route {
    pub app: App,
    pub method: String,
    pub path: String,
    pub handler: Option<String>,
    pub last: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub(crate) struct Mount {
    pub parent: App,
    pub child: App,
    pub path: String,
}

pub(crate) fn method(value: &str) -> Option<&'static str> {
    match value {
        "get" | "GET" => Some("GET"),
        "post" | "POST" => Some("POST"),
        "put" | "PUT" => Some("PUT"),
        "patch" | "PATCH" => Some("PATCH"),
        "delete" | "DELETE" => Some("DELETE"),
        "head" | "HEAD" => Some("HEAD"),
        "options" | "OPTIONS" => Some("OPTIONS"),
        "all" | "ALL" => Some("ALL"),
        _ => None,
    }
}
