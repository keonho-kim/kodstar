// Purpose: htmx의 템플릿·JSX·정적 HTML 응답과 비활성·동적 속성 경계 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실제 요청 사실과 원본 위치를 검증.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/tests/engine.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn templates_extract_static_requests_and_preserve_original_locations() -> TestResult {
    for (file, source) in [
        (
            "Page.vue",
            "<script>function target(){}</script>\n<template><button hx-get='/fragment'/></template>",
        ),
        (
            "Page.svelte",
            "<script>function target(){}</script>\n<button hx-get='/fragment'/ >",
        ),
        (
            "Page.astro",
            "---\nconst name = 'page';\n---\n<button hx-get='/fragment'></button>",
        ),
        (
            "Page.ejs",
            "<% const name = 'page'; %>\n<button hx-get='/fragment'></button>",
        ),
        (
            "Page.blade.php",
            "{{-- <b hx-get='/comment'> --}}\n<button hx-get='/fragment'></button>",
        ),
        (
            "Page.razor",
            "@page \"/page\"\n<button hx-get='/fragment'></button>",
        ),
        (
            "Page.tsx",
            "export function Page(){return <button hx-get='/fragment'/>}",
        ),
    ] {
        let p = Project::new()?;
        p.write(file, source)?;
        p.build(false)?;
        let owner = if file.ends_with("tsx") { "Page" } else { file };
        let rows = p.query(Query::Explain {
            selector: selector(file, owner),
        })?;
        let requests: Vec<_> = relationships(&rows)
            .into_iter()
            .filter(|r| r.relation == "http_request")
            .collect();
        assert_eq!(requests.len(), 1, "{file}: {requests:?}");
        let span = requests[0].site.span;
        assert_eq!(
            &source.as_bytes()[span.start_byte..span.end_byte],
            b"/fragment",
            "{file}"
        );
    }
    Ok(())
}

#[test]
fn disabled_subtrees_and_dynamic_jsx_values_do_not_claim_static_requests() -> TestResult {
    for (file, source, owner) in [
        (
            "index.html",
            "<div hx-disable><button hx-get='/disabled'></button></div><button hx-get='/active'></button>",
            "index.html",
        ),
        (
            "Page.tsx",
            "export function Page(){return <><div hx-disable><button hx-get='/disabled'/></div><button hx-get={path}/><button hx-get='/active'/></>}",
            "Page",
        ),
    ] {
        let p = Project::new()?;
        p.write(file, source)?;
        p.build(false)?;
        let rows = p.query(Query::Explain {
            selector: selector(file, owner),
        })?;
        let requests: Vec<_> = relationships(&rows)
            .into_iter()
            .filter(|r| r.relation == "http_request")
            .collect();
        assert!(
            !requests.iter().any(|r| r.expression.contains("disabled")),
            "{file}: {requests:?}"
        );
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.expression == "GET /active")
                .count(),
            1
        );
        if file.ends_with("tsx") {
            assert!(requests.iter().any(|r| r.expression == "GET <dynamic>"));
        }
    }
    Ok(())
}

#[test]
fn static_html_hono_responses_keep_request_ownership_with_the_handler() -> TestResult {
    let p = Project::new()?;
    p.write("server.ts", "import {Hono} from 'hono'; const app = new Hono();\nexport function page(c){return c.html(`<button hx-get=\"/fragment\">load</button>`);}\nexport function fragment(c){return c.text('ok')}\napp.get('/', page);app.get('/fragment', fragment);\n")?;
    p.build(false)?;
    let mut rows = Collected::default();
    p.engine.query(
        &p.options(),
        &Query::Deps {
            selector: selector("server.ts", "page"),
        },
        &QueryOptions {
            refresh: Refresh::None,
            include_candidates: true,
            ..QueryOptions::default()
        },
        &Cancellation::default(),
        &mut rows,
    )?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|r| r.relation == "http_request"
                && r.target.as_ref().is_some_and(|t| t.name == "fragment"))
    );
    Ok(())
}
