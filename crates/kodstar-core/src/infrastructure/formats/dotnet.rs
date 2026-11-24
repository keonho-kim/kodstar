// Purpose: 빌린 XML 요소에서 정적 .NET SDK·Compile 항목과 불확실성 추출.
// Pattern: Format Adapter.
// Usage: XML adapter가 csproj element마다 호출.
// Related: crates/kodstar-core/src/infrastructure/formats/xml.rs, crates/kodstar-core/src/application/analysis/dotnet_patterns.rs.

use super::xml::{positions::Positions, tag_name, values};
use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use roxmltree::Node;

fn attribute(node: Node<'_, '_>, name: &str, source: &[u8]) -> Option<String> {
    node.attributes().find_map(|attribute| {
        let (key, _) = values::attribute(attribute, source).ok()?;
        (key == name).then(|| attribute.value().to_owned())
    })
}

pub(super) fn visit(
    node: Node<'_, '_>,
    source: &[u8],
    positions: &Positions<'_>,
    emit: &mut dyn FnMut(Fact) -> Result<()>,
) -> Result<()> {
    if !node.is_element() {
        return Ok(());
    }
    let tag = tag_name(node, source);
    let mut ancestor = Some(node);
    let mut uncertain = false;
    let mut runtime = false;
    while let Some(parent) = ancestor {
        if parent.is_element() {
            uncertain |= attribute(parent, "Condition", source).is_some();
            runtime |= tag_name(parent, source) == "Target";
        }
        ancestor = parent.parent();
    }
    let mut binding = |kind, name: String, value: Option<String>| {
        emit(Fact::Binding(Binding {
            scope: 0,
            name,
            target: None,
            module: value,
            imported_name: None,
            type_name: (uncertain || runtime).then(|| "uncertain".into()),
            kind,
            span: positions.span(node.range()),
        }))
    };
    match tag {
        "Project" => {
            if let Some(sdk) = attribute(node, "Sdk", source) {
                binding("dotnet_sdk", "@sdk".into(), Some(sdk))?;
            }
        }
        "Sdk" => binding("dotnet_sdk", "@sdk".into(), attribute(node, "Name", source))?,
        "Import" | "Choose" => binding("dotnet_dynamic", tag.into(), None)?,
        "EnableDefaultCompileItems" | "EnableDefaultItems" => {
            let value = values::content(node, source).map(|text| text.trim().to_owned());
            binding("dotnet_default", tag.into(), value)?;
        }
        "DefaultItemExcludes"
        | "DefaultExcludesInProjectFolder"
        | "BaseOutputPath"
        | "BaseIntermediateOutputPath" => {
            binding("dotnet_dynamic", tag.into(), None)?;
        }
        "Compile" => {
            for (attribute_name, kind) in [
                ("Include", "dotnet_compile_include"),
                ("Exclude", "dotnet_compile_exclude"),
                ("Remove", "dotnet_compile_remove"),
            ] {
                if let Some(value) = attribute(node, attribute_name, source) {
                    binding(kind, node.range().start.to_string(), Some(value))?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
