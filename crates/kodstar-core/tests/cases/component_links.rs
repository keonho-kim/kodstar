// Purpose: 파일 간 component·정적 event와 코드/주석 문자열의 오인식 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/src/infrastructure/formats/components.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn implicit_component_defaults_link_template_tags_to_imported_files() -> TestResult {
    for (file, child, source) in [
        (
            "Main.vue",
            "Panel.vue",
            "<script setup>import MyPanel from './Panel.vue';</script>\n<template><my-panel /></template>",
        ),
        (
            "Main.svelte",
            "Panel.svelte",
            "<script>import Panel from './Panel.svelte';</script>\n<Panel />",
        ),
        (
            "Main.astro",
            "Panel.astro",
            "---\nimport Panel from './Panel.astro';\n---\n<Panel />",
        ),
    ] {
        let p = Project::new()?;
        p.write(child, "<div>child</div>")?;
        p.write(file, source)?;
        p.build(false)?;
        let rows = p.query(Query::Explain {
            selector: selector(file, file),
        })?;
        let edge = relationships(&rows)
            .into_iter()
            .find(|e| {
                e.relation == "references" && e.target.as_ref().is_some_and(|t| t.file == child)
            })
            .ok_or_else(|| format!("{file}: {:?}", rows.rows))?;
        assert_eq!(edge.resolution, "resolved");
        assert_eq!(
            edge.site.span.line,
            if file.ends_with("astro") { 4 } else { 2 }
        );
    }
    Ok(())
}

#[test]
fn jsx_tags_reference_imported_components_and_namespace_members() -> TestResult {
    let p = Project::new()?;
    p.write("Panel.tsx", "export default function Panel() { return <div/> }\nexport function Button() { return <button/> }\n")?;
    p.write("Main.tsx", "import Panel from './Panel'; import * as UI from './Panel';\nexport function Main(){ return <><Panel/><UI.Button/><div/></> }\n")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.tsx", "Main"),
    })?;
    for name in ["Panel", "Button"] {
        assert!(
            relationships(&rows)
                .iter()
                .any(|e| e.relation == "references"
                    && e.resolution == "resolved"
                    && e.target
                        .as_ref()
                        .is_some_and(|t| t.name == name && t.file == "Panel.tsx")),
            "{:?}",
            rows.rows
        );
    }
    assert!(!relationships(&rows).iter().any(|e| e.expression == "div"));
    Ok(())
}

#[test]
fn static_vue_and_svelte_event_handlers_keep_original_use_ranges() -> TestResult {
    for (file, source) in [
        (
            "Main.vue",
            "<script setup>function clicked() {}</script>\n<template><button @click=\"clicked\">ok</button></template>",
        ),
        (
            "Main.svelte",
            "<script>function clicked() {}</script>\n<button on:click={clicked}>ok</button><button onclick={clicked}>new</button>",
        ),
    ] {
        let p = Project::new()?;
        p.write(file, source)?;
        p.build(false)?;
        let rows = p.query(Query::Explain {
            selector: selector(file, file),
        })?;
        let edges = relationships(&rows);
        let handlers: Vec<_> = edges
            .iter()
            .filter(|e| {
                e.relation == "event_handler"
                    && e.target.as_ref().is_some_and(|t| t.name == "clicked")
            })
            .collect();
        assert_eq!(
            handlers.len(),
            if file.ends_with("vue") { 1 } else { 2 },
            "{:?}",
            rows.rows
        );
        for edge in handlers {
            assert_eq!(
                &source[edge.site.span.start_byte..edge.site.span.end_byte],
                "clicked"
            );
        }
    }
    Ok(())
}

#[test]
fn component_scanning_ignores_tag_shapes_inside_code_style_comments_and_attributes() -> TestResult {
    let p = Project::new()?;
    p.write("Panel.vue", "<div/>")?;
    let source = "<script setup>import Panel from './Panel.vue'; const text='<Panel/>';</script>\n<!-- <Panel/> --><style>.x:after{content:'<Panel/>'}</style>\n<template><div title=\"<Panel/>\">text</div><Panel/></template>";
    p.write("Main.vue", source)?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("Main.vue", "Main.vue"),
    })?;
    let edges = relationships(&rows);
    let refs: Vec<_> = edges
        .iter()
        .filter(|e| {
            e.relation == "references" && e.target.as_ref().is_some_and(|t| t.file == "Panel.vue")
        })
        .collect();
    assert_eq!(refs.len(), 1, "{:?}", rows.rows);
    assert_eq!(
        refs[0].site.span.start_byte,
        source.rfind("<Panel/>").ok_or("tag")? + 1
    );
    Ok(())
}

#[test]
fn interpolation_strings_and_script_shaped_attributes_do_not_create_code_regions() -> TestResult {
    for (file, source) in [
        (
            "Main.vue",
            "<script setup>import Panel from './Panel.vue';</script>\n<style>.x:after{content:'<script>fake</script>'}</style>\n<template><div title=\"<script>bad</script>\">{{ '<Panel/>' }}</div><Panel/></template>",
        ),
        (
            "Main.svelte",
            "<script>import Panel from './Panel.svelte';</script>\n<div>{'<Panel/>'}</div><Panel/>",
        ),
        (
            "Main.astro",
            "---\nimport Panel from './Panel.astro';\n---\n<div>{'<Panel/>'}</div><Panel/>",
        ),
    ] {
        let p = Project::new()?;
        let child = file.replace("Main", "Panel");
        p.write(&child, "<div/>")?;
        p.write(file, source)?;
        p.build(false)?;
        let rows = p.query(Query::Explain {
            selector: selector(file, file),
        })?;
        let refs: Vec<_> = relationships(&rows)
            .into_iter()
            .filter(|e| {
                e.relation == "references" && e.target.as_ref().is_some_and(|t| t.file == child)
            })
            .collect();
        assert_eq!(refs.len(), 1, "{file}: {:?}", rows.rows);
        assert_eq!(
            refs[0].site.span.start_byte,
            source.rfind("<Panel/>").ok_or("tag")? + 1
        );
    }
    Ok(())
}

#[test]
fn a_new_component_target_reresolves_unchanged_templates_like_a_full_build() -> TestResult {
    let p = Project::new()?;
    p.write(
        "Main.svelte",
        "<script>import Panel from './Panel.svelte';</script><Panel/>",
    )?;
    p.build(false)?;
    p.write("Panel.svelte", "<div/>")?;
    assert_eq!(p.build(false)?.parsed_files, 1);
    let query = || Query::Explain {
        selector: selector("Main.svelte", "Main.svelte"),
    };
    let rows = p.query(query())?;
    let normalized = |rows: &Collected| {
        relationships(rows)
            .iter()
            .map(|e| {
                (
                    e.relation.clone(),
                    e.resolution.clone(),
                    e.site.span.start_byte,
                    e.target.as_ref().map(|t| t.logical_key.clone()),
                )
            })
            .collect::<Vec<_>>()
    };
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.relation == "references" && e.resolution == "resolved")
    );
    p.build(true)?;
    assert_eq!(normalized(&rows), normalized(&p.query(query())?));
    assert_eq!(p.build(false)?.parsed_files, 0);
    Ok(())
}
