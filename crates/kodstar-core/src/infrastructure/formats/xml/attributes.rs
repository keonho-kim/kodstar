// Purpose: XML 속성의 파일·타입·이벤트 관계와 XAML 소유 클래스 상태.
// Pattern: Format Projection.
// Usage: XML 순회가 요소의 속성을 원문 순서대로 전달.
// Related: crates/kodstar-core/src/infrastructure/formats/xml.rs, crates/kodstar-core/src/infrastructure/formats/xml/positions.rs.

use super::{positions::Positions, values};
use crate::domain::{
    diagnostics::Result,
    facts::{Fact, SyntaxUse},
    relations::RelationKind,
};
use std::path::Path;

#[derive(Default)]
pub(super) struct Attributes {
    root_class: String,
}

impl Attributes {
    pub fn visit(
        &mut self,
        node: roxmltree::Node<'_, '_>,
        path: &Path,
        source: &[u8],
        positions: &Positions<'_>,
        emit: &mut dyn FnMut(Fact) -> Result<()>,
    ) -> Result<()> {
        for attribute in node.attributes() {
            let (name, value_range) = values::attribute(attribute, source)?;
            let value_text = attribute.value();
            let owner = super::tag_name(node, source);
            let relation = if name == "Include"
                && matches!(
                    owner,
                    "ProjectReference" | "Compile" | "Page" | "ApplicationDefinition"
                )
                || name == "Path" && owner == "Project"
                || name == "Value" && owner == "Filename"
            {
                Some(RelationKind::DependsOn)
            } else if name == "Source" && owner == "ResourceDictionary" {
                Some(RelationKind::TemplateInclude)
            } else {
                None
            };
            if let Some(relation) = relation {
                emit(Fact::Use(SyntaxUse {
                    source: 0,
                    scope: 0,
                    expression: format!("@file:{}", value_text.replace('\\', "/")),
                    relation,
                    span: positions.span(value_range.clone()),
                    condition: None,
                    type_only: false,
                }))?;
            }
            if name == "x:Class" {
                self.root_class.clear();
                self.root_class.push_str(value_text);
                emit(Fact::Use(SyntaxUse {
                    source: 0,
                    scope: 0,
                    expression: value_text.to_owned(),
                    relation: RelationKind::UsesType,
                    span: positions.span(value_range.clone()),
                    condition: None,
                    type_only: false,
                }))?;
                let filename = path.file_name().unwrap_or_default().to_string_lossy();
                emit(Fact::Use(SyntaxUse {
                    source: 0,
                    scope: 0,
                    expression: format!("@file:{filename}.cs"),
                    relation: RelationKind::Includes,
                    span: positions.span(value_range.clone()),
                    condition: None,
                    type_only: false,
                }))?;
            }
            if matches!(
                name,
                "Click"
                    | "Loaded"
                    | "Unloaded"
                    | "TextChanged"
                    | "SelectionChanged"
                    | "Checked"
                    | "Unchecked"
                    | "Closing"
                    | "Opened"
                    | "KeyDown"
                    | "KeyUp"
                    | "MouseDown"
                    | "MouseUp"
                    | "Tapped"
            ) && !self.root_class.is_empty()
                && value_text
                    .chars()
                    .all(|character| character.is_alphanumeric() || character == '_')
            {
                emit(Fact::Use(SyntaxUse {
                    source: 0,
                    scope: 0,
                    expression: format!("{}::{value_text}", self.root_class),
                    relation: RelationKind::EventHandler,
                    span: positions.span(value_range.clone()),
                    condition: None,
                    type_only: false,
                }))?;
            }
        }
        Ok(())
    }
}
