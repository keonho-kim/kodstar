// Purpose: XML project·resource·event 관계 추출.
// Pattern: Format Adapter.
// Usage: auxiliary가 XML 보조 입력을 전달.
// Related: crates/kodstar-core/src/infrastructure/formats/auxiliary/mod.rs.

use std::path::Path;

use tree_sitter::Node;

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact, SyntaxUse};
use crate::domain::relations::RelationKind;
use crate::infrastructure::parse::{ParseSession, child_kind, span, text};

pub(super) fn tag_name<'a>(node: Node<'_>, source: &'a [u8]) -> &'a str {
    let tag = if node.kind() == "element" {
        child_kind(node, &["STag", "EmptyElemTag"])
    } else {
        Some(node)
    };
    tag.and_then(|tag| child_kind(tag, &["Name"]))
        .map(|name| text(name, source))
        .unwrap_or("")
}

fn element_text(node: Node<'_>, name: &str, source: &[u8]) -> Option<String> {
    let content = child_kind(node, &["content"])?;
    let mut cursor = content.walk();
    let child = content
        .named_children(&mut cursor)
        .find(|child| child.kind() == "element" && tag_name(*child, source) == name)?;
    let content = child_kind(child, &["content"])?;
    let mut cursor = content.walk();
    let mut value = String::new();
    for part in content
        .named_children(&mut cursor)
        .filter(|node| node.kind() == "CharData")
    {
        value.push_str(text(part, source));
    }
    Some(value.trim().to_string())
}

pub(super) fn decode(value: &str) -> String {
    value
        .trim_matches(['\'', '"'])
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

pub(crate) fn extract(
    parser: &mut ParseSession,
    path: &Path,
    source: &[u8],
    cancel: &Cancellation,
    emit: &mut dyn FnMut(Fact) -> Result<()>,
) -> Result<()> {
    let tree = parser.parse_tree(&tree_sitter_xml::LANGUAGE_XML.into(), path, source, cancel)?;
    if path.extension().is_some_and(|s| s == "csproj") {
        emit(Fact::Binding(Binding {
            scope: 0,
            name: "@project".into(),
            target: None,
            module: path.parent().and_then(Path::to_str).map(str::to_owned),
            imported_name: None,
            type_name: None,
            kind: "dotnet_project",
            span: span(tree.root_node()),
        }))?;
    }
    let mut cursor = tree.root_node().walk();
    let mut root_class = String::new();
    let mut depth = 0usize;
    loop {
        cancel.check()?;
        let node = cursor.node();
        if depth > 1024 {
            return Err(crate::domain::diagnostics::Error::operation(
                "XML nesting exceeds 1024 levels",
            ));
        }
        if path.extension().is_some_and(|s| s == "csproj") {
            super::dotnet::visit(node, source, emit)?;
        }
        if node.kind() == "element"
            && tag_name(node, source) == "dependency"
            && let (Some(group), Some(artifact)) = (
                element_text(node, "groupId", source),
                element_text(node, "artifactId", source),
            )
        {
            emit(Fact::Use(SyntaxUse {
                source: 0,
                scope: 0,
                expression: format!("@package:maven:{group}:{artifact}"),
                relation: RelationKind::DependsOn,
                span: span(node),
                condition: None,
                type_only: false,
            }))?;
        }
        if node.kind() == "Attribute"
            && let (Some(name), Some(value)) =
                (child_kind(node, &["Name"]), child_kind(node, &["AttValue"]))
        {
            let name = text(name, source);
            let value_text = decode(text(value, source));
            let owner = node
                .parent()
                .map(|node| tag_name(node, source))
                .unwrap_or("");
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
                    span: span(value),
                    condition: None,
                    type_only: false,
                }))?;
            }
            if name == "x:Class" {
                root_class = value_text.clone();
                emit(Fact::Use(SyntaxUse {
                    source: 0,
                    scope: 0,
                    expression: value_text.clone(),
                    relation: RelationKind::UsesType,
                    span: span(value),
                    condition: None,
                    type_only: false,
                }))?;
                let filename = path.file_name().unwrap_or_default().to_string_lossy();
                emit(Fact::Use(SyntaxUse {
                    source: 0,
                    scope: 0,
                    expression: format!("@file:{filename}.cs"),
                    relation: RelationKind::Includes,
                    span: span(value),
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
            ) && !root_class.is_empty()
                && value_text
                    .chars()
                    .all(|character| character.is_alphanumeric() || character == '_')
            {
                emit(Fact::Use(SyntaxUse {
                    source: 0,
                    scope: 0,
                    expression: format!("{root_class}::{value_text}"),
                    relation: RelationKind::EventHandler,
                    span: span(value),
                    condition: None,
                    type_only: false,
                }))?;
            }
        }
        if cursor.goto_first_child() {
            depth += 1;
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return Ok(());
            }
            depth = depth.saturating_sub(1);
        }
    }
}
