// Purpose: 보조 입력의 명시적 파일·루트 경로·패키지 참조 해석.
// Pattern: Simple Module.
// Usage: 저장된 fact를 해석할 때 Lookup이 marker를 전달.
// Related: crates/kodstar-core/src/application/analysis/resolve/mod.rs, crates/kodstar-core/src/infrastructure/formats/auxiliary/mod.rs.

use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::infrastructure::cache::paths::path_bytes;
use std::path::{Component, Path, PathBuf};

pub(super) enum Disposition {
    Unhandled,
    Handled,
    Discarded,
}

impl Lookup<'_, '_> {
    pub(super) fn input(&mut self, context: &FileContext, expression: &str) -> Result<Disposition> {
        if let Some(result) = self.clang_call(context)? {
            return Ok(result);
        }
        if let Some(result) = self.http_input(context, expression)? {
            return Ok(result);
        }
        if matches!(
            context.language,
            crate::domain::languages::Language::C | crate::domain::languages::Language::Cpp
        ) && expression.starts_with("@native-macro:")
        {
            self.rule = "native_macro_syntax";
            self.reason = Some("native macro expansion is not evaluated");
            return Ok(Disposition::Handled);
        }
        if let Some(result) = self.ejs_input(context, expression)? {
            return Ok(result);
        }
        if context.language == crate::domain::languages::Language::Php
            && self.php_import_reference(context, expression)?
        {
            return Ok(Disposition::Handled);
        }
        if let Some(component) = expression.strip_prefix("@livewire-component:") {
            self.external_target(&format!("livewire:component:{component}"))?;
            self.rule = "explicit_livewire_component";
            self.reason = Some("component registration and PHP owner are not established");
        } else if expression.starts_with("@livewire-action:") {
            self.rule = "explicit_livewire_action";
            self.reason = Some("Livewire action has no established component owner");
        } else if expression.starts_with("@blade-view:") {
            self.rule = "explicit_blade_view";
            self.reason = Some("Blade view is dynamic or uses an unsupported registration/path");
        } else if let Some(route) = expression.strip_prefix("@route:") {
            self.external_target(&format!("route:{route}"))?;
            self.rule = "static_route_declaration";
        } else if let Some(tag) = expression.strip_prefix("@component:") {
            let (scope, position): (u32, usize) = self.connection.cached_row(
                "SELECT scope_id,start_byte FROM facts WHERE id=?1",
                [self.fact],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let name = if context.path.extension().is_some_and(|s| s == "vue") && tag.contains('-')
            {
                let mut name = String::with_capacity(tag.len());
                for part in tag.split('-') {
                    let mut characters = part.chars();
                    if let Some(first) = characters.next() {
                        name.extend(first.to_uppercase());
                        name.extend(characters);
                    }
                }
                name
            } else {
                tag.to_owned()
            };
            self.expression(context, scope, position, &name, 0)?;
            self.razor_component_file(context, tag)?;
        } else if let Some(name) = expression
            .strip_prefix("@lisp-symbol:")
            .filter(|_| context.language == crate::domain::languages::Language::CommonLisp)
        {
            let (scope, position): (u32, usize) = self.connection.cached_row(
                "SELECT scope_id,start_byte FROM facts WHERE id=?1",
                [self.fact],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            self.expression(context, scope, position, name, 0)?;
        } else if let Some(mixin) = expression.strip_prefix("@ruby-mixin:") {
            if let Some((_, name)) = mixin.split_once('#') {
                let (scope, position): (u32, usize) = self.connection.cached_row(
                    "SELECT scope_id,start_byte FROM facts WHERE id=?1",
                    [self.fact],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                self.type_expression(context, scope, position, name, 0)?;
            }
        } else if let Some(package) = expression.strip_prefix("@lisp-package:") {
            self.lisp_package_reference(context, package)?;
        } else if let Some(property) = expression.strip_prefix("@property:") {
            self.python_property(context, property)?;
        } else if let Some(configuration) = expression.strip_prefix("@configuration:") {
            if let Some((kind, raw)) = configuration.split_once(':') {
                let parent = context.path.parent().unwrap_or_else(|| Path::new(""));
                if let Some(file) =
                    crate::application::analysis::resolve::indexed_paths::configuration_file(
                        self.connection,
                        parent,
                        raw,
                        kind,
                    )?
                {
                    self.connection.cached_execute("INSERT OR IGNORE INTO resolution_candidates SELECT id FROM symbols WHERE file_id=?1 AND kind='file'",[file])?;
                    self.rule = "explicit_configuration_reference";
                } else {
                    self.reason = Some("configuration is external, excluded or missing");
                }
            }
        } else if let Some(implementation) = expression.strip_prefix("@impl:") {
            if let Some((owner, contract)) = implementation.split_once('#') {
                let (scope, position): (u32, usize) = self.connection.cached_row(
                    "SELECT scope_id,start_byte FROM facts WHERE id=?1",
                    [self.fact],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                self.expression(context, scope, position, owner, 0)?;
                self.source_override=self.connection.cached_row("SELECT CASE WHEN COUNT(*)=1 THEN MIN(c.id) END FROM resolution_candidates c CROSS JOIN symbols s ON s.id=c.id WHERE s.kind IN ('class','struct','enum','type')",[],|r|r.get(0))?;
                self.connection
                    .cached_execute("DELETE FROM resolution_candidates", [])?;
                if self.source_override.is_some() {
                    self.expression(context, scope, position, contract, 0)?;
                    self.rule = "explicit_trait_implementation";
                } else {
                    self.reason = Some("implementation owner has no unique identified type");
                }
            }
        } else if let Some(import) = expression.strip_prefix("@require:") {
            let (scope, position): (u32, usize) = self.connection.cached_row(
                "SELECT scope_id,start_byte FROM facts WHERE id=?1",
                [self.fact],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            if self.binding(context, scope, position, "require")?.is_none() {
                let (module, name) = import
                    .split_once('#')
                    .map_or((import, None), |(m, n)| (m, Some(n)));
                self.module(context, scope, module, name, 0)?;
            } else {
                self.reason = Some("lexical binding shadows CommonJS require");
            }
        } else if let Some(package) = expression.strip_prefix("@package:") {
            self.rule = "manifest_dependency";
            self.external_target(&format!("package:{package}"))?;
            self.reason = Some("declared dependency; external package contents are not indexed");
            self.dependency("package", package, true)?;
        } else if let Some(local) = expression.strip_prefix("@local:") {
            if let Ok(local) = local.parse::<u32>() {
                self.connection.cached_execute("INSERT OR IGNORE INTO resolution_candidates SELECT id FROM symbols WHERE file_id=?1 AND local_id=?2", rusqlite::params![context.id,local])?;
                self.rule = "syntax_containment";
            }
        } else if let Some(file) = expression.strip_prefix("@packagefile:") {
            let parent = context.path.parent().unwrap_or_else(|| Path::new(""));
            if file.starts_with("./")
                && crate::application::analysis::resolve::indexed_paths::normalize(parent, file)
                    .is_some_and(|p| p.starts_with(parent))
            {
                self.file(parent, file)?;
            } else {
                self.reason = Some("package export target escapes its package or is not relative");
            }
        } else if let Some(file) = expression.strip_prefix("@file:") {
            let parent = context.path.parent().unwrap_or_else(|| Path::new(""));
            self.file(parent, file)?;
        } else if let Some(file) = expression.strip_prefix("@rootfile:") {
            self.file(Path::new(""), file)?;
        } else {
            return Ok(Disposition::Unhandled);
        }
        Ok(Disposition::Handled)
    }

    fn file(&mut self, parent: &Path, raw: &str) -> Result<()> {
        self.rule = "explicit_file_reference";
        if raw.is_empty() || raw.contains(['*', '?', '$', '{', '}', '\0']) {
            self.reason =
                Some("file reference contains an unsupported variable, glob, or empty path");
            self.dependency("file_expression", raw, false)?;
            return Ok(());
        }
        let mut path = PathBuf::new();
        for component in parent.join(raw).components() {
            match component {
                Component::Normal(name) => path.push(name),
                Component::CurDir => {}
                Component::ParentDir if path.pop() => {}
                _ => {
                    self.reason = Some("file reference escapes the scanned project root");
                    self.dependency("file_expression", raw, false)?;
                    return Ok(());
                }
            }
        }
        // Lookup only the enumerated index. Resolving a reference never opens an arbitrary path,
        // follows a symlink, runs a build, or fetches a missing dependency.
        let raw_path = path_bytes(&path);
        self.connection.cached_execute(
            "INSERT OR IGNORE INTO resolution_candidates SELECT s.id FROM symbols s JOIN files f ON f.id=s.file_id WHERE f.path=?1 AND s.kind='file'",
            [&raw_path],
        )?;
        let found: bool = self.connection.cached_row(
            "SELECT EXISTS(SELECT 1 FROM resolution_candidates)",
            [],
            |row| row.get(0),
        )?;
        self.dependency("file", &blake3::hash(&raw_path).to_hex(), found)?;
        if !found {
            self.reason =
                Some("explicit file target is missing, excluded, skipped, or unsupported");
        }
        Ok(())
    }
}
