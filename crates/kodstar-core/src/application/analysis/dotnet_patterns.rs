// Purpose: 프로젝트 하나의 정적 Compile 순서와 제한된 MSBuild glob 평가.
// Pattern: Simple Module.
// Usage: dotnet_projects가 프로젝트별로 생성하고 파일을 스트리밍해 대조.
// Related: crates/kodstar-core/src/infrastructure/formats/dotnet.rs, crates/kodstar-core/src/application/analysis/dotnet_projects.rs.

use super::resolve::indexed_paths::normalize;
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use rusqlite::{Connection, params};
use std::path::Path;

struct Operation {
    include: bool,
    matches: GlobSet,
    excludes: GlobSet,
}

pub(super) struct Plan {
    defaults: bool,
    operations: Vec<Operation>,
    pub uncertain: bool,
}

impl Plan {
    pub fn read(
        c: &Connection,
        project: i64,
        directory: &Path,
        cancel: &Cancellation,
    ) -> Result<Self> {
        let mut plan = Self {
            defaults: false,
            operations: Vec::new(),
            uncertain: false,
        };
        let mut items = true;
        let mut compile = true;
        let mut budget = PatternBudget { bytes: 0, count: 0 };
        let mut statement = c.prepare("SELECT kind,name,module_name,type_name,start_byte FROM bindings WHERE file_id=?1 AND kind LIKE 'dotnet_%' ORDER BY start_byte,id")?;
        let mut rows = statement.query([project])?;
        while let Some(row) = rows.next()? {
            cancel.check()?;
            let kind: String = row.get(0)?;
            let name: String = row.get(1)?;
            let value: Option<String> = row.get(2)?;
            let conditional: Option<String> = row.get(3)?;
            let position: i64 = row.get(4)?;
            let value = value.as_deref().unwrap_or("");
            plan.uncertain |= conditional.is_some();
            match kind.as_str() {
                "dotnet_sdk" => {
                    let known = value.split(';').all(|s| {
                        matches!(
                            s.trim(),
                            "Microsoft.NET.Sdk"
                                | "Microsoft.NET.Sdk.Web"
                                | "Microsoft.NET.Sdk.Razor"
                                | "Microsoft.NET.Sdk.Worker"
                                | "Microsoft.NET.Sdk.WindowsDesktop"
                                | "Microsoft.NET.Sdk.BlazorWebAssembly"
                        )
                    });
                    plan.defaults |= known;
                    plan.uncertain |= !known;
                }
                "dotnet_default" => {
                    let enabled = match value.to_ascii_lowercase().as_str() {
                        "true" => true,
                        "false" => false,
                        _ => {
                            plan.uncertain = true;
                            continue;
                        }
                    };
                    if name == "EnableDefaultItems" {
                        items = enabled;
                    } else {
                        compile = enabled;
                    }
                }
                "dotnet_dynamic" => plan.uncertain = true,
                "dotnet_compile_include" | "dotnet_compile_remove" => {
                    if value.trim().is_empty() {
                        continue;
                    }
                    let Some(matches) = budget.compile(directory, value) else {
                        plan.uncertain = true;
                        continue;
                    };
                    let mut excludes = GlobSetBuilder::new().build().map_err(|e| {
                        crate::domain::diagnostics::Error::caused_by("empty Compile exclusions", e)
                    })?;
                    if kind == "dotnet_compile_include" {
                        let excluded: Option<String> = c.query_row("SELECT module_name FROM bindings WHERE file_id=?1 AND kind='dotnet_compile_exclude' AND start_byte=?2 LIMIT 1", params![project, position], |r| r.get(0)).optional()?.flatten();
                        if let Some(value) = excluded {
                            if let Some(compiled) = budget.compile(directory, &value) {
                                excludes = compiled;
                            } else {
                                plan.uncertain = true;
                            }
                        }
                    }
                    plan.operations.push(Operation {
                        include: kind == "dotnet_compile_include",
                        matches,
                        excludes,
                    });
                }
                _ => {}
            }
        }
        plan.defaults &= items && compile;
        Ok(plan)
    }

    pub fn contains(&self, directory: &Path, path: &Path) -> bool {
        let mut included = self.defaults
            && path.strip_prefix(directory).is_ok_and(|relative| {
                relative.extension().is_some_and(|s| s == "cs")
                    && !relative.components().any(|part| {
                        let value = part.as_os_str().to_string_lossy();
                        value == "bin" || value == "obj" || value.starts_with('.')
                    })
            });
        for operation in &self.operations {
            if operation.matches.is_match(path) && !operation.excludes.is_match(path) {
                included = operation.include;
            }
        }
        included
    }
}

use rusqlite::OptionalExtension;

struct PatternBudget {
    bytes: usize,
    count: usize,
}
impl PatternBudget {
    fn compile(&mut self, directory: &Path, value: &str) -> Option<GlobSet> {
        let mut builder = GlobSetBuilder::new();
        for pattern in value.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            self.bytes = self.bytes.checked_add(pattern.len())?;
            self.count = self.count.checked_add(1)?;
            if self.bytes > 65_536 || self.count > 1024 || pattern.contains(['$', '@', '%', '&']) {
                return None;
            }
            let mut wildcard = false;
            for part in pattern.split(['/', '\\']) {
                if part == ".." && wildcard {
                    return None;
                }
                wildcard |= part.contains(['*', '?']);
            }
            let normalized = normalize(directory, &pattern.replace('\\', "/"))?;
            let mut escaped = String::new();
            for character in normalized.to_str()?.chars() {
                if matches!(character, '[' | ']' | '{' | '}' | '\\') {
                    escaped.push('\\');
                }
                escaped.push(character);
            }
            builder.add(
                GlobBuilder::new(&escaped)
                    .literal_separator(true)
                    .backslash_escape(true)
                    .build()
                    .ok()?,
            );
        }
        builder.build().ok()
    }
}
