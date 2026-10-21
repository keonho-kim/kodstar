// Purpose: XML AST에서 정적 .NET SDK·Compile 항목과 불확실성 추출.
// Pattern: Format Adapter.
// Usage: XML adapter가 csproj element마다 호출.
// Related: crates/kodstar-core/src/infrastructure/formats/xml.rs, crates/kodstar-core/src/application/analysis/dotnet_patterns.rs.

use super::xml::{decode, tag_name};
use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::infrastructure::parse::{child_kind, span, text};
use tree_sitter::Node;

fn attribute(node: Node<'_>, name: &str, source: &[u8]) -> Option<String> {
    let tag = child_kind(node, &["STag", "EmptyElemTag"])?;
    let mut cursor = tag.walk();
    for attribute in tag
        .named_children(&mut cursor)
        .filter(|n| n.kind() == "Attribute")
    {
        if child_kind(attribute, &["Name"]).is_some_and(|n| text(n, source) == name) {
            return child_kind(attribute, &["AttValue"]).map(|n| decode(text(n, source)));
        }
    }
    None
}

pub(super) fn visit(
    node: Node<'_>,
    source: &[u8],
    emit: &mut dyn FnMut(Fact) -> Result<()>,
) -> Result<()> {
    if node.kind() != "element" {
        return Ok(());
    }
    let tag = tag_name(node, source);
    let mut ancestor = Some(node);
    let mut uncertain = false;
    let mut runtime = false;
    while let Some(parent) = ancestor {
        if parent.kind() == "element" {
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
            span: span(node),
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
            let value = child_kind(node, &["content"]).map(|n| text(n, source).trim().to_owned());
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
                    binding(kind, node.start_byte().to_string(), Some(value))?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
