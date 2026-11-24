// Purpose: XML adapter의 원문 위치·조건 상속·닫힌 entity 입력 회귀.
// Pattern: Unit Test.
// Usage: core tests가 별도 파일 I/O 없이 보존 XML 입력을 추출.
// Related: crates/kodstar-core/src/infrastructure/formats/xml.rs, crates/kodstar-core/src/infrastructure/formats/dotnet.rs.

use super::*;
use crate::domain::source::SourceSpan;

#[test]
fn long_attribute_spacing_preserves_unicode_quotes_and_conditional_compile_spans() -> Result<()> {
    let source = format!(
        "<Project Sdk='Microsoft.NET.Sdk'>\r\n<ItemGroup Condition='optional'>\r\n<Compile Include{}=\"한글.cs\"/>\r\n</ItemGroup></Project>",
        " ".repeat(512)
    );
    let mut parser = ParseSession::new(32768);
    let mut facts = Vec::new();
    extract(
        &mut parser,
        Path::new("app/app.csproj"),
        source.as_bytes(),
        &Cancellation::default(),
        &mut |fact| {
            facts.push(fact);
            Ok(())
        },
    )?;
    let value = "\"한글.cs\"";
    let start = source.find(value).unwrap();
    assert!(facts.iter().any(|fact| matches!(fact, Fact::Use(usage)
        if usage.expression == "@file:한글.cs" && usage.span == SourceSpan::at(source.as_bytes(), start, start + value.len()))));
    assert!(facts.iter().any(|fact| matches!(fact, Fact::Binding(binding)
        if binding.kind == "dotnet_compile_include" && binding.module.as_deref() == Some("한글.cs") && binding.type_name.as_deref() == Some("uncertain"))));
    Ok(())
}

#[test]
fn maven_dependency_has_the_complete_original_element_range() -> Result<()> {
    let source = "<project>\n<dependency><groupId> org.example </groupId><artifactId>engine</artifactId></dependency>\n</project>";
    let mut parser = ParseSession::new(1);
    let mut facts = Vec::new();
    extract(
        &mut parser,
        Path::new("pom.xml"),
        source.as_bytes(),
        &Cancellation::default(),
        &mut |fact| {
            facts.push(fact);
            Ok(())
        },
    )?;
    let start = source.find("<dependency>").unwrap();
    let end = source.find("</dependency>").unwrap() + "</dependency>".len();
    assert!(facts.iter().any(|fact| matches!(fact, Fact::Use(usage)
        if usage.expression == "@package:maven:org.example:engine" && usage.span == SourceSpan::at(source.as_bytes(), start, end))));
    Ok(())
}

#[test]
fn unresolved_external_entities_fail_without_publishing_an_xml_result() {
    let source = b"<!DOCTYPE project [<!ENTITY outside SYSTEM 'file:///__kodstar_missing_secret'>]><project>&outside;</project>";
    let mut parser = ParseSession::new(32768);
    let mut facts = Vec::new();
    assert!(
        extract(
            &mut parser,
            Path::new("pom.xml"),
            source,
            &Cancellation::default(),
            &mut |fact| {
                facts.push(fact);
                Ok(())
            }
        )
        .is_err()
    );
    assert!(facts.is_empty());
}

#[test]
fn xml_entity_values_are_decoded_while_spans_still_cover_the_original_text() -> Result<()> {
    let source = "<Project><Compile Include='A&#x26;B.cs'/><dependency><groupId>org&#46;example</groupId><artifactId><![CDATA[engine]]></artifactId></dependency></Project>";
    let mut parser = ParseSession::new(32768);
    let mut facts = Vec::new();
    extract(
        &mut parser,
        Path::new("pom.xml"),
        source.as_bytes(),
        &Cancellation::default(),
        &mut |fact| {
            facts.push(fact);
            Ok(())
        },
    )?;
    let start = source.find("'A&#x26;B.cs'").unwrap();
    assert!(facts.iter().any(|fact| matches!(fact, Fact::Use(usage) if usage.expression == "@file:A&B.cs" && usage.span.start_byte == start && usage.span.end_byte == start + "'A&#x26;B.cs'".len())));
    assert!(facts.iter().any(|fact| matches!(fact, Fact::Use(usage) if usage.expression == "@package:maven:org.example:engine")));
    Ok(())
}

#[test]
fn utf8_bom_is_outside_the_xml_project_binding_but_inside_the_file_input() -> Result<()> {
    let source = "\u{feff}<Project Sdk='Microsoft.NET.Sdk'/>\n";
    let mut parser = ParseSession::new(32768);
    let mut facts = Vec::new();
    extract(
        &mut parser,
        Path::new("app.csproj"),
        source.as_bytes(),
        &Cancellation::default(),
        &mut |fact| {
            facts.push(fact);
            Ok(())
        },
    )?;
    assert!(facts.iter().any(|fact| matches!(fact, Fact::Binding(binding) if binding.kind == "dotnet_project" && binding.span == SourceSpan::at(source.as_bytes(), 3, source.len()))));
    Ok(())
}
