// Purpose: 독립된 Hono import 조회의 순서·worker와 증분 일치 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 검사가 여러 앱의 mounted handler를 조회.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/tests/engine.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn independent_imported_apps_do_not_share_module_candidates() -> TestResult {
    for order in [["users", "posts"], ["posts", "users"]] {
        let p = Project::new()?;
        for name in ["users", "posts"] {
            p.write(&format!("{name}.ts"), &format!("import {{Hono}} from 'hono';export const {name}=new Hono();export function {name}Handler(c){{return c.text('ok')}}{name}.get('/{name}',{name}Handler);"))?;
        }
        p.write("server.ts", &format!("import {{Hono}} from 'hono';import {{users}} from './users';import {{posts}} from './posts';const app=new Hono();app.route('/api',{});app.route('/api',{});",order[0],order[1]))?;
        p.write(
            "index.html",
            "<button hx-get='/api/users'></button><button hx-get='/api/posts'></button>",
        )?;
        p.build(false)?;
        let mut rows = Collected::default();
        p.engine.query(
            &p.options(),
            &Query::Deps {
                selector: selector("index.html", "index.html"),
            },
            &QueryOptions {
                refresh: Refresh::None,
                include_candidates: true,
                limit: Limit::Unlimited,
                ..QueryOptions::default()
            },
            &Cancellation::default(),
            &mut rows,
        )?;
        for name in ["usersHandler", "postsHandler"] {
            assert!(
                relationships(&rows)
                    .iter()
                    .any(|r| r.relation == "http_request"
                        && r.target.as_ref().is_some_and(|t| t.name == name)),
                "{order:?}: {rows:?}"
            );
        }
        p.write("users.ts", "import {Hono} from 'hono';export const users=new Hono();export function usersHandler(c){return c.text('changed')}users.get('/users',usersHandler);")?;
        let updated = p.build(false)?;
        assert_eq!(
            snapshot::normalized_index(&updated)?,
            snapshot::normalized_index(&p.build(true)?)?
        );
    }
    Ok(())
}
