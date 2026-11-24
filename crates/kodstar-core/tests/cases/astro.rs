// Purpose: Astro 서버·브라우저 scope와 processed/inline script·template import 경계 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/src/infrastructure/formats/components.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn astro_server_and_client_functions_do_not_inherit_each_other() -> TestResult {
    let p = Project::new()?;
    p.write("Main.astro", "---\nfunction serverOnly(){} function serverCaller(){clientOnly();}\n---\n<script>function clientOnly(){} function clientCaller(){serverOnly();}</script>")?;
    p.build(false)?;
    for name in ["serverCaller", "clientCaller"] {
        let rows = p.query(Query::Explain {
            selector: selector("Main.astro", name),
        })?;
        assert!(
            !relationships(&rows)
                .iter()
                .any(|e| e.relation == "calls" && e.target.is_some()),
            "{name}: {:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn astro_processed_scripts_are_independent_typescript_modules_and_inline_globals_are_shared()
-> TestResult {
    let p = Project::new()?;
    p.write("Main.astro", "<script is:inline>function shared(){}</script>\n<script>const value: number=1; function privateOnly(){} function own(){privateOnly(); shared();}</script>\n<script>function sibling(){privateOnly(); shared();}</script>\n<script is:inline>function classic(){shared(); privateOnly();}</script>")?;
    p.build(false)?;
    for (caller, private) in [("own", true), ("sibling", false), ("classic", false)] {
        let rows = p.query(Query::Explain {
            selector: selector("Main.astro", caller),
        })?;
        assert_eq!(
            relationships(&rows).iter().any(|e| e.relation == "calls"
                && e.target.as_ref().is_some_and(|t| t.name == "privateOnly")),
            private,
            "{:?}",
            rows.rows
        );
        assert!(
            relationships(&rows)
                .iter()
                .any(|e| e.relation == "calls"
                    && e.target.as_ref().is_some_and(|t| t.name == "shared")),
            "{:?}",
            rows.rows
        );
    }
    Ok(())
}

#[test]
fn astro_template_components_use_server_imports_while_client_imports_keep_their_own_scope()
-> TestResult {
    let p = Project::new()?;
    p.write("Server.astro", "<div/>")?;
    p.write("Client.ts", "export function Client(){}")?;
    p.write("Main.astro", "---\nimport Server from './Server.astro';\n---\n<script>import {Client} from './Client'; function boot(){Client();}</script>\n<Server/><Client/>")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("Main.astro", "Main.astro"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.relation == "references"
                && e.target.as_ref().is_some_and(|t| t.file == "Server.astro")),
        "{:?}",
        rows.rows
    );
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.relation == "references"
                && e.target.as_ref().is_some_and(|t| t.file == "Client.ts")),
        "{:?}",
        rows.rows
    );
    let rows = p.query(Query::Deps {
        selector: selector("Main.astro", "boot"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.relation == "calls"
                && e.target.as_ref().is_some_and(|t| t.file == "Client.ts")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn astro_script_slices_keep_original_ranges_and_cannot_complete_each_others_syntax() -> TestResult {
    let p = Project::new()?;
    let source = "---\r\nconst title='한글';\r\n---\r\n<script>function target(){} function boot(){target();}</script>";
    p.write("Main.astro", source)?;
    let before = p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.astro", "boot"),
    })?;
    let edges = relationships(&rows);
    let edge = edges.iter().find(|e| e.relation == "calls").ok_or("call")?;
    assert_eq!(
        &source[edge.site.span.start_byte..edge.site.span.end_byte],
        "target"
    );
    assert_eq!(edge.site.span.line, 4);
    p.write(
        "Main.astro",
        "<script>function broken(){</script><script>}</script>",
    )?;
    assert_eq!(p.build(false)?.coverage.partial_files, 1);
    let rows = p.query(Query::Symbols {
        file: "Main.astro".into(),
    })?;
    assert_eq!(
        rows.metadata.ok_or("metadata")?.generation,
        Some(before.generation + 1)
    );
    Ok(())
}

#[test]
fn astro_browser_exports_do_not_replace_the_component_default_or_escape_as_server_exports()
-> TestResult {
    let p = Project::new()?;
    p.write("Child.astro", "<script>export default function Browser(){} export function browserOnly(){}</script><div/>")?;
    p.write("Main.astro", "---\nimport Child, {browserOnly} from './Child.astro'; function server(){browserOnly();}\n---\n<Child/>")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("Main.astro", "Main.astro"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.relation == "references"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.file == "Child.astro" && t.kind == "file")),
        "{:?}",
        rows.rows
    );
    let rows = p.query(Query::Explain {
        selector: selector("Main.astro", "server"),
    })?;
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.relation == "calls" && e.target.is_some()),
        "{:?}",
        rows.rows
    );
    Ok(())
}
