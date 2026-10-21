// Purpose: Blade literal·주석·PHP 영역과 Livewire 이름·action 해석 경계 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/src/infrastructure/formats/components/blade/mod.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn blade_dynamic_directives_do_not_capture_later_strings_or_literal_prefixes() -> TestResult {
    let p = Project::new()?;
    p.write("Main.blade.php", "@include($name)<div title=\"unrelated\"></div>\n@include('partial' . $suffix)\n@includeIf('optional')\n@includeWhen($ok, 'conditional')")?;
    for name in ["unrelated", "partial", "optional", "conditional"] {
        p.write(&format!("resources/views/{name}.blade.php"), "<p/>")?;
    }
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("Main.blade.php", "Main.blade.php"),
    })?;
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e
                .target
                .as_ref()
                .is_some_and(|t| t.file.ends_with("unrelated.blade.php")
                    || t.file.ends_with("partial.blade.php")
                    || t.file.ends_with("conditional.blade.php"))),
        "{:?}",
        rows.rows
    );
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.relation == "template_include" && e.resolution == "unresolved"),
        "{:?}",
        rows.rows
    );
    assert!(
        relationships(&rows).iter().any(|e| e
            .target
            .as_ref()
            .is_some_and(|t| t.file.ends_with("optional.blade.php"))),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn blade_comments_verbatim_escaped_directives_and_php_strings_are_not_template_code() -> TestResult
{
    let p = Project::new()?;
    p.write("Main.blade.php", "{{-- @include('fake') @php function fake(){} @endphp <livewire:fake/> --}}\n@verbatim @include('fake') @php function fake2(){} @endphp <livewire:fake/> @endverbatim\n@@include('fake')\n@php $text=\"@include('fake')\"; function real(){} @endphp")?;
    p.write("resources/views/fake.blade.php", "<p/>")?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "Main.blade.php".into(),
    })?;
    assert!(
        rows.rows
            .iter()
            .any(|r| matches!(r,QueryRow::Symbol(s) if s.name=="real")),
        "{:?}",
        rows.rows
    );
    assert!(
        !rows
            .rows
            .iter()
            .any(|r| matches!(r,QueryRow::Symbol(s) if s.name.starts_with("fake"))),
        "{:?}",
        rows.rows
    );
    let rows = p.query(Query::Explain {
        selector: selector("Main.blade.php", "Main.blade.php"),
    })?;
    assert!(
        !relationships(&rows)
            .iter()
            .any(|e| e.relation == "template_include"
                || e.external
                    .as_deref()
                    .is_some_and(|s| s.starts_with("livewire:"))),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn blade_static_view_directives_preserve_original_ranges_and_missing_targets_reresolve()
-> TestResult {
    let p = Project::new()?;
    let source = "한글\r\n@extends('layouts.app')\n@component('parts.card', ['key'=>'value'])";
    p.write("Main.blade.php", source)?;
    p.write("resources/views/layouts/app.blade.php", "<main/>")?;
    p.build(false)?;
    p.write("resources/views/parts/card.blade.php", "<article/>")?;
    assert_eq!(p.build(false)?.parsed_files, 1);
    let query = || Query::Explain {
        selector: selector("Main.blade.php", "Main.blade.php"),
    };
    let rows = p.query(query())?;
    for (relation, name, line) in [
        ("template_extends", "layouts.app", 2),
        ("template_include", "parts.card", 3),
    ] {
        let edges = relationships(&rows);
        let edge = edges
            .iter()
            .find(|e| e.relation == relation && e.resolution == "resolved")
            .ok_or("view relation")?;
        assert_eq!(
            &source[edge.site.span.start_byte..edge.site.span.end_byte],
            name
        );
        assert_eq!(edge.site.span.line, line);
    }
    let normalized = |rows: &Collected| {
        relationships(rows)
            .iter()
            .map(|e| {
                (
                    e.relation.clone(),
                    e.resolution.clone(),
                    e.target.as_ref().map(|t| t.logical_key.clone()),
                )
            })
            .collect::<Vec<_>>()
    };
    p.build(true)?;
    assert_eq!(normalized(&rows), normalized(&p.query(query())?));
    Ok(())
}

#[test]
fn livewire_components_and_click_actions_keep_framework_identity_without_global_name_guessing()
-> TestResult {
    let p = Project::new()?;
    let source = "<livewire:posts.create-post/><button wire:click.prevent=\"save(42)\">Save</button><button wire:click=\"{{ $action }}\">Dynamic</button><script>const text='<livewire:fake/><b wire:click=\"fake\">';</script><!-- <livewire:fake/> -->";
    p.write("Main.blade.php", source)?;
    p.write(
        "Other.php",
        "<?php class Unrelated { public function save(){} }",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("Main.blade.php", "Main.blade.php"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.relation == "references"
                && e.external.as_deref() == Some("livewire:component:posts.create-post")),
        "{:?}",
        rows.rows
    );
    let edges = relationships(&rows);
    let save = edges
        .iter()
        .find(|e| {
            e.relation == "event_handler"
                && &source[e.site.span.start_byte..e.site.span.end_byte] == "save"
        })
        .ok_or("save action")?;
    assert_eq!(save.resolution, "unresolved");
    assert!(save.target.is_none());
    assert!(
        !edges
            .iter()
            .any(|e| e.target.as_ref().is_some_and(|t| t.file == "Other.php")
                || e.external.as_deref() == Some("livewire:component:fake")),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn blade_php_and_directive_arguments_cannot_start_fake_comments_or_nested_template_directives()
-> TestResult {
    let p = Project::new()?;
    p.write("Main.blade.php", "@php $text='{{--'; @endphp\n@include($name, ['text'=>\"@include('fake')\"])\n{{ \"<livewire:fake/>\" }}\n@php($x = 1)\n@php function real(){} @endphp")?;
    p.write("resources/views/fake.blade.php", "<p/>")?;
    let before = p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("Main.blade.php", "Main.blade.php"),
    })?;
    assert!(
        !relationships(&rows).iter().any(|e| e
            .target
            .as_ref()
            .is_some_and(|t| t.file.ends_with("fake.blade.php"))
            || e.external.as_deref() == Some("livewire:component:fake")),
        "{:?}",
        rows.rows
    );
    p.write("Main.blade.php", "{{-- incomplete")?;
    assert!(p.build(false).is_err());
    let rows = p.query(Query::Symbols {
        file: "Main.blade.php".into(),
    })?;
    assert_eq!(
        rows.metadata.ok_or("metadata")?.generation,
        Some(before.generation)
    );
    Ok(())
}

#[test]
fn blade_php_attributes_are_not_comments_and_php_close_terminates_line_comments() -> TestResult {
    let p = Project::new()?;
    p.write("Main.blade.php", "<?php // comment containing */ ?>@php #[Attribute] function attributed(){} @endphp <?php # other comment ?>@php function after(){} @endphp")?;
    p.build(false)?;
    let rows = p.query(Query::Symbols {
        file: "Main.blade.php".into(),
    })?;
    for name in ["attributed", "after"] {
        assert!(
            rows.rows
                .iter()
                .any(|r| matches!(r, QueryRow::Symbol(s) if s.name==name)),
            "{name}: {:?}",
            rows.rows
        );
    }
    Ok(())
}
