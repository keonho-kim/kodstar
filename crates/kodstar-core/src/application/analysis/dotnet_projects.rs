// Purpose: 정적 .NET Compile 소속·프로젝트 중복·XAML 문맥을 세대 내 계산.
// Pattern: Coordinator.
// Usage: 분석 transaction이 partial 정체성과 참조 해석 전에 실행.
// Related: crates/kodstar-core/src/application/analysis/dotnet_patterns.rs, crates/kodstar-core/src/infrastructure/formats/dotnet.rs.

use super::dotnet_patterns::Plan;
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::infrastructure::cache::paths::{cache_key, path_bytes, path_from_bytes};
use rusqlite::{Connection, params};
use std::path::Path;

pub(super) fn run(c: &Connection, root: &Path, cancel: &Cancellation) -> Result<()> {
    c.execute_batch("CREATE TEMP TABLE IF NOT EXISTS dotnet_membership(file_id INTEGER PRIMARY KEY,merge_allowed INTEGER);
        CREATE TEMP TABLE IF NOT EXISTS dotnet_coverage(file_id INTEGER PRIMARY KEY);
        CREATE TEMP TABLE IF NOT EXISTS dotnet_candidates(file_id INTEGER,project INTEGER,uncertain INTEGER,PRIMARY KEY(file_id,project));
        DELETE FROM dotnet_membership; DELETE FROM dotnet_candidates; DELETE FROM dotnet_coverage;
        DELETE FROM diagnostics WHERE code IN ('dotnet_membership_uncertain','dotnet_membership_ambiguous');")?;
    stage(c, cancel)?;
    let mut statement = c.prepare("SELECT id,path,path_package FROM files WHERE language IN ('csharp','razor') OR (language='auxiliary' AND display_path LIKE '%.xaml') ORDER BY path")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let file: i64 = row.get(0)?;
        let raw: Vec<u8> = row.get(1)?;
        let base: String = row.get(2)?;
        let (count, uncertain, project, covered): (u64,bool,Option<Vec<u8>>,bool) = c.query_row("SELECT COUNT(*),COALESCE(MAX(d.uncertain),0),MIN(f.path),EXISTS(SELECT 1 FROM dotnet_coverage WHERE file_id=?1) FROM dotnet_candidates d JOIN files f ON f.id=d.project WHERE d.file_id=?1", [file], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
        let allowed = !uncertain && count <= 1 && (count == 1 || !covered);
        let package = if !allowed {
            format!(
                "{}:dotnet-unassigned:{}",
                cache_key(root),
                blake3::hash(&raw).to_hex()
            )
        } else if let Some(project) = project {
            cache_key(&root.join(path_from_bytes(&project)?))
        } else {
            base
        };
        c.execute(
            "UPDATE files SET package=?1 WHERE id=?2",
            params![package, file],
        )?;
        c.execute(
            "INSERT INTO dotnet_membership VALUES(?1,?2)",
            params![file, allowed],
        )?;
        if count > 1 {
            c.execute("INSERT INTO diagnostics(file_id,code,message) VALUES(?1,'dotnet_membership_ambiguous','source belongs to multiple indexed projects; per-project source instances are not modeled and partial types remain separate')", [file])?;
        }
    }
    Ok(())
}

fn stage(c: &Connection, cancel: &Cancellation) -> Result<()> {
    let mut statement = c.prepare(
        "SELECT id,path,status FROM files WHERE display_path LIKE '%.csproj' ORDER BY path",
    )?;
    let mut projects = statement.query([])?;
    while let Some(project) = projects.next()? {
        cancel.check()?;
        let id: i64 = project.get(0)?;
        let raw: Vec<u8> = project.get(1)?;
        let status: String = project.get(2)?;
        let path = path_from_bytes(&raw)?;
        let directory = path.parent().unwrap_or_else(|| Path::new(""));
        let mut plan = Plan::read(c, id, directory, cancel)?;
        plan.uncertain |=
            !matches!(status.as_str(), "analyzed" | "auxiliary") || customized(c, directory)?;
        if plan.uncertain {
            c.execute("INSERT INTO diagnostics(file_id,code,message) VALUES(?1,'dotnet_membership_uncertain','Compile membership contains conditional, dynamic, imported, custom SDK or over-budget rules; build files were not executed and partial types remain separate')", [id])?;
        }
        let mut files = c.prepare("SELECT id,path,language FROM files WHERE language IN ('csharp','razor') OR (language='auxiliary' AND display_path LIKE '%.xaml') ORDER BY path")?;
        let mut rows = files.query([])?;
        while let Some(row) = rows.next()? {
            cancel.check()?;
            let file: i64 = row.get(0)?;
            let raw: Vec<u8> = row.get(1)?;
            let language: String = row.get(2)?;
            let path = path_from_bytes(&raw)?;
            let covered = path.starts_with(directory);
            if covered {
                c.execute("INSERT OR IGNORE INTO dotnet_coverage VALUES(?1)", [file])?;
            }
            if (language != "csharp" || plan.uncertain) && covered
                || language == "csharp" && plan.contains(directory, &path)
            {
                c.execute(
                    "INSERT INTO dotnet_candidates VALUES(?1,?2,?3)",
                    params![file, id, plan.uncertain],
                )?;
            }
        }
    }
    Ok(())
}

fn customized(c: &Connection, directory: &Path) -> Result<bool> {
    for ancestor in directory.ancestors() {
        for name in ["Directory.Build.props", "Directory.Build.targets"] {
            let present: bool = c.query_row(
                "SELECT EXISTS(SELECT 1 FROM files WHERE path=?1)",
                [path_bytes(&ancestor.join(name))],
                |r| r.get(0),
            )?;
            if present {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
