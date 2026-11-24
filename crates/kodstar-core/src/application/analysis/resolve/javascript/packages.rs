// Purpose: JS/TS package exports·imports와 명시적 로컬 entry 해석.
// Pattern: Simple Module.
// Usage: module_files가 설정 alias로 처리하지 못한 specifier를 전달.
// Related: crates/kodstar-core/src/application/analysis/resolve/javascript/workspaces.rs, crates/kodstar-core/src/application/analysis/resolve/javascript/targets.rs.

use crate::application::analysis::resolve::modules::Location;
use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::domain::resolve::path_patterns::match_pattern;
use rusqlite::params;

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn javascript_packages(
        &mut self,
        context: &FileContext,
        scope: u32,
        raw: &str,
        depth: usize,
    ) -> Result<Location> {
        if raw.starts_with("node:") {
            return Ok(Location::External(raw.into()));
        }
        if raw.starts_with('#') {
            return self.package_import(context, scope, raw, depth);
        }
        let Some((name, subpath)) = package_name(raw) else {
            return Ok(Location::Unmapped);
        };
        let declared = self.select_packages(context, name, depth)?;
        self.dependency("package", name, declared)?;
        let mut packages = self.connection.cached_prepare(
            "SELECT manifest FROM resolution_package_candidates WHERE depth=?1 ORDER BY manifest",
        )?;
        let records = packages.query_map([depth], |r| r.get::<_, i64>(0))?;
        for manifest in records {
            self.package_exports(context, manifest?, &subpath, depth)?;
        }
        Ok(if declared {
            Location::Project
        } else {
            Location::Unmapped
        })
    }

    fn package_import(
        &mut self,
        context: &FileContext,
        scope: u32,
        raw: &str,
        depth: usize,
    ) -> Result<Location> {
        let Some((manifest, directory)) = self.nearest_package(context)? else {
            return Ok(Location::Project);
        };
        let Some(pattern) = self.package_pattern(manifest, "package_import", raw)? else {
            return Ok(Location::Project);
        };
        self.package_options(context, manifest, "package_import", &pattern, depth)?;
        let captured = match_pattern(raw, &pattern)
            .map(|m| m.captured)
            .unwrap_or("");
        let mut entries=self.connection.cached_prepare("SELECT target,native FROM resolution_package_entries WHERE depth=?1 ORDER BY priority,position")?;
        let records = entries.query_map([depth], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?))
        })?;
        for target in records {
            let (target, native) = target?;
            let target = target.replacen('*', captured, 1);
            if target.starts_with('.') {
                if self.package_source(&directory, &target, true, depth)? {
                    if native {
                        self.platform_candidate();
                    }
                    return Ok(Location::Project);
                }
            } else {
                let location = self.module_files(context, scope, &target, depth + 1)?;
                if native {
                    self.platform_candidate();
                }
                self.connection.cached_execute("INSERT OR IGNORE INTO resolution_modules SELECT ?1,id FROM resolution_modules WHERE depth=?2",params![depth,depth+1])?;
                return Ok(if matches!(location, Location::Unmapped) {
                    Location::External(target)
                } else {
                    location
                });
            }
        }
        Ok(Location::Project)
    }
}

fn package_name(raw: &str) -> Option<(&str, String)> {
    let end = if raw.starts_with('@') {
        let scope = raw.find('/')?;
        raw[scope + 1..]
            .find('/')
            .map_or(raw.len(), |n| scope + 1 + n)
    } else {
        raw.find('/').unwrap_or(raw.len())
    };
    let name = &raw[..end];
    if name.is_empty() {
        return None;
    }
    let rest = raw[end..].trim_start_matches('/');
    Some((
        name,
        if rest.is_empty() {
            ".".into()
        } else {
            format!("./{rest}")
        },
    ))
}
