// Purpose: Elixir alias 묶음·only/except import와 정적 use 사실 추출.
// Pattern: Simple Module.
// Usage: import dispatcher가 Elixir call 노드를 전달.
// Related: crates/kodstar-core/src/infrastructure/parse/imports.rs, crates/kodstar-core/src/application/analysis/resolve/scope_imports.rs.

use crate::domain::diagnostics::Result;
use crate::domain::facts::{Binding, Fact, SyntaxUse};
use crate::domain::relations::RelationKind;
use crate::infrastructure::parse::imports::Imports;
use crate::infrastructure::parse::{child_kind, span, text};
use tree_sitter::Node;

impl Imports<'_> {
    pub(in crate::infrastructure::parse) fn elixir(&mut self, node: Node<'_>) -> Result<()> {
        let Some(target) = node.child_by_field_name("target") else {
            return Ok(());
        };
        let target = text(target, self.source);
        if !matches!(target, "alias" | "import" | "use" | "require") {
            return Ok(());
        }
        let Some(args) = child_kind(node, &["arguments"]) else {
            return Ok(());
        };
        let Some(module) = args.named_child(0) else {
            return Ok(());
        };
        let options = child_kind(args, &["keywords"]);
        if target == "alias"
            && module.kind() == "dot"
            && let (Some(base), Some(group)) = (
                module.child_by_field_name("left"),
                module
                    .child_by_field_name("right")
                    .filter(|n| n.kind() == "tuple"),
            )
        {
            let prefix = text(base, self.source);
            let mut names = group.walk();
            for child in group
                .named_children(&mut names)
                .filter(|n| n.kind() == "alias")
            {
                let name = text(child, self.source);
                let path = format!("{prefix}.{name}");
                self.bind(
                    name.rsplit('.').next().unwrap_or(name),
                    &path,
                    None,
                    span(child),
                )?;
            }
            return Ok(());
        }
        if module.kind() != "alias"
            && !(module.kind() == "identifier" && text(module, self.source) == "__MODULE__")
        {
            return Ok(());
        }
        let path = text(module, self.source);
        if matches!(target, "use" | "require") {
            self.bind("_", path, None, span(module))?;
            if target == "require" {
                return Ok(());
            }
        }
        if target == "use" {
            return (self.emit)(Fact::Use(SyntaxUse {
                source: self.owner,
                scope: self.scope,
                expression: format!("@module:{path}#__using__/1"),
                relation: RelationKind::Calls,
                span: span(module),
                condition: None,
                type_only: false,
            }));
        }
        if target == "import" {
            if let Some(only) = option(options, "only:", self.source).filter(|n| n.kind() == "list")
            {
                return self.elixir_selection(only, path, false);
            }
            if let Some(except) =
                option(options, "except:", self.source).filter(|n| n.kind() == "list")
            {
                self.elixir_selection(except, path, true)?;
            }
            return self.bind("*", path, None, span(module));
        }
        let alias = option(options, "as:", self.source)
            .map(|n| text(n, self.source))
            .unwrap_or_else(|| path.rsplit('.').next().unwrap_or(path));
        self.bind(alias, path, None, span(module))
    }

    fn elixir_selection(&mut self, list: Node<'_>, module: &str, except: bool) -> Result<()> {
        let Some(keywords) = child_kind(list, &["keywords"]) else {
            return Ok(());
        };
        let mut cursor = keywords.walk();
        for pair in keywords
            .named_children(&mut cursor)
            .filter(|n| n.kind() == "pair")
        {
            let (Some(key), Some(value)) = (
                pair.child_by_field_name("key"),
                pair.child_by_field_name("value"),
            ) else {
                continue;
            };
            if value.kind() != "integer" {
                continue;
            }
            let Some(arity) = text(value, self.source).parse::<usize>().ok() else {
                continue;
            };
            let name = format!(
                "{}/{arity}",
                text(key, self.source).trim().trim_end_matches(':')
            );
            if except {
                (self.emit)(Fact::Binding(Binding {
                    scope: self.scope,
                    name,
                    target: None,
                    module: Some(module.into()),
                    imported_name: None,
                    type_name: None,
                    kind: "elixir_import_exclusion",
                    span: span(pair),
                }))?;
            } else {
                self.bind(&name, module, Some(&name), span(pair))?;
            }
        }
        Ok(())
    }
}

fn option<'t>(options: Option<Node<'t>>, name: &str, source: &[u8]) -> Option<Node<'t>> {
    let options = options?;
    let mut cursor = options.walk();
    options
        .named_children(&mut cursor)
        .find(|n| {
            n.kind() == "pair"
                && n.child_by_field_name("key")
                    .is_some_and(|n| text(n, source).trim() == name)
        })
        .and_then(|n| n.child_by_field_name("value"))
}
