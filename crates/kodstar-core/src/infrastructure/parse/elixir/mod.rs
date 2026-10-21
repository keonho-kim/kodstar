// Purpose: Elixir 함수 head·arity·기본 인수·절 메타데이터 추출.
// Pattern: Simple Module.
// Usage: 선언·사용 추출기가 Elixir call과 문맥을 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/forms.rs, crates/kodstar-core/src/infrastructure/parse/references.rs.

pub(super) mod containers;
pub(super) mod imports;

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact};
use crate::domain::languages::Language;
use crate::domain::relations::RelationKind;
use crate::infrastructure::parse::{Context, Extractor, child_kind, span, text};
use tree_sitter::Node;

pub(in crate::infrastructure::parse) struct Head<'t> {
    pub name: Node<'t>,
    pub arity: usize,
    pub minimum: usize,
}

pub(in crate::infrastructure::parse) fn head<'t>(
    node: Node<'t>,
    source: &[u8],
) -> Option<Head<'t>> {
    let keyword = node.child_by_field_name("target")?;
    if !matches!(
        text(keyword, source),
        "def" | "defp" | "defmacro" | "defmacrop" | "defguard" | "defguardp" | "defdelegate"
    ) {
        return None;
    }
    let mut body = child_kind(node, &["arguments"])?.named_child(0)?;
    while body.kind() == "binary_operator" {
        body = body
            .child_by_field_name("left")
            .or_else(|| body.named_child(0))?;
    }
    let name = if body.kind() == "call" {
        body.child_by_field_name("target")?
    } else {
        body
    };
    if name.kind() != "identifier" {
        return None;
    }
    let args = child_kind(body, &["arguments"]);
    let arity = args.map_or(0, |n| n.named_child_count());
    let mut defaults = 0;
    if let Some(args) = args {
        let mut cursor = args.walk();
        for arg in args.named_children(&mut cursor) {
            if arg.kind() == "binary_operator"
                && arg
                    .child_by_field_name("operator")
                    .is_some_and(|n| text(n, source) == "\\\\")
            {
                defaults += 1;
            }
        }
    }
    Some(Head {
        name,
        arity,
        minimum: arity - defaults,
    })
}

pub(in crate::infrastructure::parse) fn declaration_head(
    mut node: Node<'_>,
    source: &[u8],
) -> bool {
    while let Some(parent) = node.parent().filter(|n| n.kind() == "binary_operator") {
        if !parent
            .child_by_field_name("left")
            .is_some_and(|n| n.id() == node.id())
        {
            return false;
        }
        node = parent;
    }
    let Some(args) = node.parent().filter(|n| n.kind() == "arguments") else {
        return false;
    };
    args.named_child(0).is_some_and(|n| n.id() == node.id())
        && args.parent().is_some_and(|n| head(n, source).is_some())
}

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn elixir_binding_name(
        &self,
        node: Node<'_>,
        name: &str,
    ) -> String {
        head(node, self.source).map_or_else(|| name.to_owned(), |h| format!("{name}/{}", h.arity))
    }

    pub(in crate::infrastructure::parse) fn elixir_callable(
        &mut self,
        node: Node<'_>,
        scope: u32,
        context: &Context,
    ) -> Result<()> {
        if self.language != Language::Elixir {
            return Ok(());
        }
        let Some(head) = head(node, self.source) else {
            return Ok(());
        };
        let name = text(head.name, self.source);
        (self.emit)(Fact::Binding(Binding {
            scope: context.scope,
            name: "@elixir-function".into(),
            target: Some(context.source),
            module: Some(head.arity.to_string()),
            imported_name: Some(head.minimum.to_string()),
            type_name: None,
            kind: "elixir_function",
            span: span(node),
        }))?;
        for arity in head.minimum..head.arity {
            (self.emit)(Fact::Binding(Binding {
                scope,
                name: format!("{name}/{arity}"),
                target: Some(context.source),
                module: None,
                imported_name: None,
                type_name: None,
                kind: "function",
                span: span(head.name),
            }))?;
        }
        if node
            .child_by_field_name("target")
            .is_some_and(|n| text(n, self.source) == "defdelegate")
            && let Some(keywords) =
                child_kind(node, &["arguments"]).and_then(|n| child_kind(n, &["keywords"]))
        {
            let mut cursor = keywords.walk();
            for pair in keywords.named_children(&mut cursor) {
                if pair
                    .child_by_field_name("key")
                    .is_some_and(|n| text(n, self.source).trim() == "to:")
                    && let Some(module) = pair
                        .child_by_field_name("value")
                        .filter(|n| n.kind() == "alias")
                {
                    self.usage(
                        context,
                        format!(
                            "@module:{}#{name}/{}",
                            text(module, self.source),
                            head.arity
                        ),
                        RelationKind::Calls,
                        span(module),
                    )?;
                }
            }
        }
        Ok(())
    }
}

pub(in crate::infrastructure::parse) fn capture_target(node: Node<'_>, source: &[u8]) -> bool {
    node.parent().is_some_and(|parent| {
        parent.kind() == "binary_operator"
            && parent
                .child_by_field_name("operator")
                .is_some_and(|n| text(n, source) == "/")
            && parent
                .child_by_field_name("left")
                .is_some_and(|n| n.id() == node.id())
            && parent.parent().is_some_and(|n| {
                n.kind() == "unary_operator"
                    && n.child_by_field_name("operator")
                        .is_some_and(|n| text(n, source) == "&")
            })
    })
}

impl Extractor<'_> {
    pub(in crate::infrastructure::parse) fn elixir_capture(
        &mut self,
        node: Node<'_>,
        context: &Context,
    ) -> Result<()> {
        if self.language != Language::Elixir
            || node.kind() != "unary_operator"
            || !node
                .child_by_field_name("operator")
                .is_some_and(|n| text(n, self.source) == "&")
        {
            return Ok(());
        }
        let Some(operand) = node
            .child_by_field_name("operand")
            .filter(|n| n.kind() == "binary_operator")
        else {
            return Ok(());
        };
        let (Some(left), Some(right)) = (
            operand.child_by_field_name("left"),
            operand.child_by_field_name("right"),
        ) else {
            return Ok(());
        };
        if !capture_target(left, self.source) || right.kind() != "integer" {
            return Ok(());
        }
        let Some(arity) = text(right, self.source).parse::<u8>().ok() else {
            return Ok(());
        };
        let callee = if left.kind() == "call" {
            left.child_by_field_name("target")
        } else if left.kind() == "identifier" {
            Some(left)
        } else {
            None
        };
        let Some(callee) = callee else {
            return Ok(());
        };
        let mut expression = self.reference_text(callee)?;
        self.append_signature(&mut expression, &["/", &arity.to_string()])?;
        self.named_callable_value(node, context, "elixir_capture", expression)
    }
}
