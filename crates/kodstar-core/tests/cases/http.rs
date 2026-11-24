// Purpose: Hono 등록·응답과 htmx 요청의 정적 연결 및 오인식 방지 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 HTTP 관계와 증분 일치를 검증.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/tests/engine.rs.

use crate::support::*;
use kodstar_core::*;

fn deps(project: &Project, file: &str, name: &str) -> Result<Collected> {
    let mut sink = Collected::default();
    project.engine.query(
        &project.options(),
        &Query::Deps {
            selector: selector(file, name),
        },
        &QueryOptions {
            refresh: Refresh::None,
            include_candidates: true,
            limit: Limit::Unlimited,
            ..QueryOptions::default()
        },
        &Cancellation::default(),
        &mut sink,
    )?;
    Ok(sink)
}

#[test]
fn hono_handlers_render_components_and_receive_htmx_requests() -> TestResult {
    let p = Project::new()?;
    p.write("Panel.tsx", "export function Panel(){return <div/>}\n")?;
    p.write("server.tsx", "import { Hono } from 'hono';\nimport { Panel } from './Panel';\nconst app = new Hono();\nexport function show(c) { return c.html(<Panel/>); }\napp.get('/fragment', show);\napp.post('/save', c => c.html(<Panel/>));\n")?;
    p.write("index.html", "<button hx-get=\"/fragment?x=1&amp;y=2\" hx-target=\"#content\">load</button><main id=\"content\"></main><form hx-post=\"/save\"></form>")?;
    p.build(false)?;
    let requests = deps(&p, "index.html", "index.html")?;
    assert!(relationships(&requests).iter().any(
        |r| r.relation == "http_request" && r.target.as_ref().is_some_and(|t| t.name == "show")
    ));
    assert!(
        relationships(&requests)
            .iter()
            .any(|r| r.relation == "http_request"
                && r.expression.starts_with("POST ")
                && r.target.is_some())
    );
    let response = deps(&p, "server.tsx", "show")?;
    assert!(
        relationships(&response).iter().any(
            |r| r.relation == "renders" && r.target.as_ref().is_some_and(|t| t.name == "Panel")
        )
    );
    let registrations = deps(&p, "server.tsx", "server.tsx")?;
    assert!(relationships(&registrations).iter().any(|r| r.relation == "handles"
        && r.target.as_ref().is_some_and(|t| t.name == "show")));
    Ok(())
}

#[test]
fn hono_origin_shadowing_and_http_methods_prevent_false_connections() -> TestResult {
    let p = Project::new()?;
    p.write("server.ts", "import { Hono as Router } from 'hono';\nconst app = new Router();\nfunction good(c){return c.text('ok')}\napp.post('/save', good);\nfunction wrong(){}\nconst fake = new Map(); fake.get('/fake', wrong);\nfunction shadow(Router) { const app = new Router(); app.get('/shadow', wrong); }\n")?;
    p.write("index.html", "<button hx-get=\"/save\"></button><button hx-get=\"/fake\"></button><button hx-get=\"/shadow\"></button>")?;
    p.build(false)?;
    let rows = deps(&p, "index.html", "index.html")?;
    assert!(
        !relationships(&rows)
            .iter()
            .any(|r| r.relation == "http_request" && r.target.is_some())
    );
    Ok(())
}

#[test]
fn markup_comments_scripts_and_raw_text_do_not_create_htmx_requests() -> TestResult {
    let p = Project::new()?;
    p.write("index.html", "<!-- <b hx-get='/comment'> --><script>const text = \"<b hx-get='/string'>\";</script><textarea><b hx-get='/text'></textarea><button data-hx-get='/real'></button>")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("index.html", "index.html"),
    })?;
    let requests: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|r| r.relation == "http_request")
        .collect();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(requests[0].expression, "GET /real");
    Ok(())
}

#[test]
fn imported_hono_apps_and_base_paths_connect_mounted_routes() -> TestResult {
    let p = Project::new()?;
    p.write("users.ts", "import {Hono} from 'hono'; export const users = new Hono().basePath('/users'); export function show(c){return c.text('ok')} users.get('/:id', show);\n")?;
    p.write("server.ts", "import {Hono} from 'hono'; import {users} from './users'; const app = new Hono().basePath('/v1'); app.route('/api', users);\n")?;
    p.write("index.html", "<button hx-get='/v1/api/users/7'></button>")?;
    p.build(false)?;
    let rows = deps(&p, "index.html", "index.html")?;
    assert!(relationships(&rows).iter().any(|r| {
        r.relation == "http_request"
            && r.target
                .as_ref()
                .is_some_and(|t| t.name == "show" && t.file == "users.ts")
    }));
    Ok(())
}

#[test]
fn imported_app_base_path_is_preserved_when_extended_locally() -> TestResult {
    let p = Project::new()?;
    p.write(
        "base.ts",
        "import {Hono} from 'hono'; export const base = new Hono().basePath('/api');",
    )?;
    p.write("server.ts", "import {base} from './base'; const app=base.basePath('/v2'); function show(c){return c.text('ok')} app.get('/item',show);")?;
    p.write("index.html", "<button hx-get='/api/v2/item'></button>")?;
    p.build(false)?;
    let rows = deps(&p, "index.html", "index.html")?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|r| r.relation == "http_request"
                && r.target.as_ref().is_some_and(|t| t.name == "show")),
        "{rows:?}"
    );
    Ok(())
}

#[test]
fn missing_routes_and_changed_handler_exports_match_full_analysis() -> TestResult {
    let p = Project::new()?;
    p.write("index.html", "<button hx-get='/fragment'></button>")?;
    p.build(false)?;
    p.write(
        "handlers.tsx",
        "export function show(c){return c.html(<div/>)}\n",
    )?;
    p.write("server.ts", "import {Hono} from 'hono'; import {show} from './handlers'; const app = new Hono(); app.get('/fragment', show);\n")?;
    let update = p.build(false)?;
    let rows = deps(&p, "index.html", "index.html")?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|r| r.relation == "http_request" && r.target.is_some())
    );
    let incremental = snapshot::normalized_index(&update)?;
    assert_eq!(incremental, snapshot::normalized_index(&p.build(true)?)?);
    p.write(
        "server.ts",
        "import {Hono} from 'hono'; const app = new Hono();\n",
    )?;
    let update = p.build(false)?;
    let rows = deps(&p, "index.html", "index.html")?;
    assert!(
        !relationships(&rows)
            .iter()
            .any(|r| r.relation == "http_request" && r.target.is_some())
    );
    assert_eq!(
        snapshot::normalized_index(&update)?,
        snapshot::normalized_index(&p.build(true)?)?
    );
    Ok(())
}

#[test]
fn middleware_candidates_follow_registration_without_claiming_execution_order() -> TestResult {
    let p = Project::new()?;
    p.write("server.ts", "import {Hono} from 'hono'; const app=new Hono(); function auth(c,next){return next()} function show(c){return c.text('ok')} app.use('/api/*',auth);app.get('/api/value',show);\n")?;
    p.write("index.html", "<button hx-get='/api/value'></button>")?;
    p.build(false)?;
    let rows = deps(&p, "index.html", "index.html")?;
    for name in ["auth", "show"] {
        assert!(
            relationships(&rows)
                .iter()
                .any(|r| r.relation == "http_request"
                    && r.target.as_ref().is_some_and(|t| t.name == name)),
            "{rows:?}"
        );
    }
    Ok(())
}

#[test]
fn a_local_module_named_hono_does_not_establish_framework_origin() -> TestResult {
    let p = Project::new()?;
    p.write(
        "tsconfig.json",
        r#"{"compilerOptions":{"baseUrl":".","paths":{"hono":["./fake.ts"]}}}"#,
    )?;
    p.write("fake.ts", "export class Hono {get(path,fn){return this}}")?;
    p.write("server.ts", "import {Hono} from 'hono';const app=new Hono();function wrong(){} app.get('/value',wrong);")?;
    p.write("index.html", "<button hx-get='/value'></button>")?;
    p.build(false)?;
    let rows = deps(&p, "index.html", "index.html")?;
    assert!(
        !relationships(&rows)
            .iter()
            .any(|r| r.relation == "http_request" && r.target.is_some())
    );
    Ok(())
}
