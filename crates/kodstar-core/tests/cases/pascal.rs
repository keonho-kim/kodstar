// Purpose: Pascal unit·분리 구현·대소문자·복수 정의 위치 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn pascal_implementation_deletion_preserves_declaration_identity_and_full_result() -> TestResult {
    let p = Project::new()?;
    let declaration = "unit Api; interface type TThing = class procedure Run; end; implementation ";
    p.write(
        "Api.pas",
        &(declaration.to_owned() + "procedure TThing.Run; begin end; end."),
    )?;
    p.write("Main.pas","unit Main; interface uses Api; implementation procedure Caller(value: TThing); begin value.Run(); end; end.")?;
    p.build(false)?;
    let query = Query::Deps {
        selector: selector("Main.pas", "Caller"),
    };
    let before = p.query(query.clone())?;
    let key = relationships(&before)
        .iter()
        .find_map(|e| {
            e.target
                .as_ref()
                .filter(|t| t.qualified_name == "TThing::Run")
                .map(|t| t.logical_key.clone())
        })
        .ok_or("missing initial method")?;
    p.write("Api.pas", &(declaration.to_owned() + "end."))?;
    assert_eq!(p.build(false)?.parsed_files, 1);
    let incremental = p.query(query.clone())?;
    assert!(
        relationships(&incremental).iter().any(|e| e
            .target
            .as_ref()
            .is_some_and(|t| t.logical_key == key && t.definitions.is_empty())),
        "{:?}",
        incremental.rows
    );
    p.build(true)?;
    let full = p.query(query)?;
    assert!(
        relationships(&full).iter().any(|e| e
            .target
            .as_ref()
            .is_some_and(|t| t.logical_key == key && t.definitions.is_empty())),
        "{:?}",
        full.rows
    );
    Ok(())
}

#[test]
fn pascal_malformed_uses_string_preserves_the_last_generation() -> TestResult {
    let p = Project::new()?;
    p.write(
        "Main.pas",
        "unit Main; interface implementation procedure Caller; begin end; end.",
    )?;
    let initial = p.build(false)?;
    p.write(
        "Main.pas",
        "unit Main; interface uses Api in 'unfinished; implementation end.",
    )?;
    assert!(p.build(false).is_err());
    let rows = p.query(Query::Symbols {
        file: "Main.pas".into(),
    })?;
    assert_eq!(
        rows.metadata.ok_or("metadata")?.generation,
        Some(initial.generation)
    );
    assert!(
        rows.rows
            .iter()
            .any(|r| matches!(r,QueryRow::Symbol(s) if s.name=="Caller"))
    );
    Ok(())
}

#[test]
fn pascal_properties_and_fields_use_declared_receiver_types() -> TestResult {
    let p = Project::new()?;
    p.write("Main.pas","unit Main; interface type TWorker = class procedure Run; end; TOwner = class worker: TWorker; function GetWorker: TWorker; property Value: TWorker read GetWorker; procedure Caller; end; implementation procedure TWorker.Run; begin end; function TOwner.GetWorker: TWorker; begin Result := nil; end; procedure TOwner.Caller; begin Self.Value.Run(); Self.worker.Run(); end; end.")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.pas", "TOwner::Caller"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e.relation == "calls"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == "TWorker::Run"))
            .map(|e| e.count)
            .sum::<u64>(),
        2,
        "{:?}",
        rows.rows
    );
    let property = p.query(Query::Deps {
        selector: selector("Main.pas", "TOwner::Value"),
    })?;
    assert!(
        relationships(&property)
            .iter()
            .any(|e| e.relation == "references"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == "TOwner::GetWorker")),
        "{:?}",
        property.rows
    );
    Ok(())
}

#[test]
fn pascal_form_events_follow_the_explicit_paired_source_file() -> TestResult {
    let p = Project::new()?;
    p.write("Ui.pas","unit DifferentUnit; interface type TMainForm = class procedure ButtonClick; end; implementation procedure TMainForm.ButtonClick; begin end; end.")?;
    p.write(
        "Ui.dfm",
        "object MainForm: TMainForm\n  OnClick = ButtonClick\nend\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Ui.dfm", "Ui.dfm"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.relation == "event_handler"
                && e.resolution == "resolved"
                && e.target.as_ref().is_some_and(
                    |t| t.file == "Ui.pas" && t.qualified_name == "TMainForm::ButtonClick"
                )),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn pascal_overloads_keep_distinct_signature_groups_and_multiple_locations() -> TestResult {
    let p = Project::new()?;
    p.write("Main.pas","unit Main; interface type TThing = class procedure Run(value: Integer); overload; procedure Run(value: String); overload; procedure Caller; end; implementation procedure TThing.Run(value: Integer); begin end; procedure TThing.Run(value: String); begin end; procedure TThing.Caller; var value: TThing; begin value.Run(1); end; end.")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("Main.pas", "TThing::Caller"),
    })?;
    let calls: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect();
    assert_eq!(calls.len(), 2, "{:?}", rows.rows);
    assert!(
        calls.iter().all(|e| e.resolution == "ambiguous"
            && e.target.as_ref().is_some_and(|t| t.definitions.len() == 2)),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn pascal_implementation_private_procedures_do_not_escape_the_unit() -> TestResult {
    let p = Project::new()?;
    p.write("Api.pas","unit Api; interface procedure PublicTarget; implementation procedure Hidden; begin end; procedure PublicTarget; begin Hidden(); end; end.")?;
    p.write("Main.pas","unit Main; interface uses Api; implementation procedure Caller; begin Api.Hidden(); Api.PublicTarget(); end; end.")?;
    p.build(false)?;
    let rows = p.query(Query::Explain {
        selector: selector("Main.pas", "Caller"),
    })?;
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "Api.Hidden" && e.resolution == "unresolved"),
        "{:?}",
        rows.rows
    );
    assert!(
        relationships(&rows)
            .iter()
            .any(|e| e.expression == "Api.PublicTarget" && e.resolution == "resolved"),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn pascal_uses_resolves_bare_and_qualified_names_with_ascii_case_insensitivity() -> TestResult {
    let p = Project::new()?;
    p.write("Api.pas","unit ApiUnit; interface procedure Target; implementation procedure Target; begin end; end.")?;
    p.write("Main.pas","unit Main; interface uses apiunit; implementation procedure Caller; begin TARGET(); APIUNIT.target(); end; end.")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.pas", "caller"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e.relation == "calls"
                && e.resolution == "resolved"
                && e.target.as_ref().is_some_and(|t| t.file == "Api.pas"))
            .map(|e| e.count)
            .sum::<u64>(),
        2,
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn pascal_split_implementation_merges_only_matching_class_and_signature() -> TestResult {
    let p = Project::new()?;
    p.write("Main.pas","unit Main; interface type TThing = class procedure Run(value: Integer); procedure Caller; end; implementation procedure TThing.Run(value: Integer); begin end; procedure TThing.Caller; var value: TThing; begin value.rUn(1); Self.RUN(2); end; end.")?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.pas", "TThing::Caller"),
    })?;
    let calls: Vec<_> = relationships(&rows)
        .into_iter()
        .filter(|e| e.relation == "calls")
        .collect();
    assert_eq!(
        calls.iter().map(|e| e.count).sum::<u64>(),
        2,
        "{:?}",
        rows.rows
    );
    assert!(
        calls.iter().all(|e| e
            .target
            .as_ref()
            .is_some_and(|t| t.qualified_name == "TThing::Run" && t.definitions.len() == 2)),
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn pascal_explicit_unit_file_preserves_original_call_and_definition_positions() -> TestResult {
    let p = Project::new()?;
    p.write("generated/한글Api.pas","unit ApiUnit; interface procedure Target; implementation procedure Target; begin end; end.")?;
    p.write("unrelated.pas","unit ApiUnit; interface procedure Target; implementation procedure Target; begin end; end.")?;
    let source = "unit Main; interface uses ApiUnit in 'generated/한글Api.pas'; implementation procedure Caller; begin ApiUnit.Target(); end; end.";
    p.write("Main.pas", source)?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("Main.pas", "Caller"),
    })?;
    let call = relationships(&rows)
        .into_iter()
        .find(|e| e.relation == "calls")
        .ok_or("missing call")?;
    assert_eq!(
        call.target.as_ref().ok_or("target")?.file,
        "generated/한글Api.pas"
    );
    assert_eq!(
        &source[call.site.span.start_byte..call.site.span.end_byte],
        "ApiUnit.Target"
    );
    Ok(())
}
