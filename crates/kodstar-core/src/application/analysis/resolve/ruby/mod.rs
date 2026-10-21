// Purpose: Ruby instance/singleton 조회와 정적 생성자·lookup barrier 적용.
// Pattern: Simple Module.
// Usage: lexical·멤버 해석과 fact 공개 전 검증이 호출.
// Related: crates/kodstar-core/src/application/analysis/resolve/ruby/members.sql, crates/kodstar-core/src/infrastructure/parse/ruby/mod.rs.

use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use rusqlite::params;

impl Lookup<'_, '_> {
    fn ruby_side(&self) -> Result<String> {
        let Some(fact) = self.fact else {
            return Ok("ambiguous".into());
        };
        let (expression,source_side):(String,String)=self.connection.query_row("SELECT f.expression,COALESCE(b.module_name,CASE WHEN s.kind IN ('class','module') THEN 'singleton' ELSE 'instance' END)
            FROM facts f
            JOIN symbols s ON s.id=f.source_id
            LEFT JOIN bindings b ON b.file_id=s.file_id
            AND b.target_local=s.local_id
            AND b.kind='ruby_dispatch'
            WHERE f.id=?1",[fact],|r|Ok((r.get(0)?,r.get(1)?)))?;
        if expression.starts_with("@result:") {
            return Ok("instance".into());
        }
        if expression.starts_with("@constructor:") {
            return Ok("singleton".into());
        }
        if let Some((receiver, _)) = expression.rsplit_once('.') {
            if receiver == "self" {
                return Ok(source_side);
            }
            return Ok(if receiver.starts_with("::")
                || receiver.chars().next().is_some_and(char::is_uppercase)
            {
                "singleton"
            } else {
                "instance"
            }
            .into());
        }
        Ok(source_side)
    }

    fn ruby_barred(&self) -> Result<bool> {
        Ok(self.connection.query_row("WITH RECURSIVE owners(id) AS (
            SELECT CASE WHEN s.kind IN ('method','function') THEN s.owner_id ELSE s.id END
            FROM resolution_candidates c JOIN symbols s ON s.id=c.id
            UNION SELECT r.target_id FROM owners o JOIN relationships r ON r.source_id=o.id JOIN facts f ON f.id=r.fact_id
              WHERE f.relation IN ('inherits','mixes_in') AND r.resolution='resolved'
          ) SELECT EXISTS(SELECT 1 FROM bindings b JOIN symbols s ON s.file_id=b.file_id AND s.local_id=b.target_local
              WHERE b.kind='ruby_lookup_barrier' AND (s.id IN (SELECT id FROM owners)
                OR s.kind='file' AND (s.file_id IN (SELECT file_id FROM symbols WHERE id IN (SELECT id FROM owners)) OR s.file_id=(SELECT file_id FROM facts WHERE id=?1))))
            OR EXISTS(SELECT 1 FROM bindings b WHERE b.kind='ruby_project_barrier')",[self.fact],|r|r.get(0))?)
    }

    pub(in crate::application::analysis::resolve) fn ruby_members(
        &mut self,
        name: &str,
    ) -> Result<()> {
        let side = self.ruby_side()?;
        let barred = self.ruby_barred()?;
        self.connection
            .execute("DELETE FROM resolution_members", [])?;
        self.connection
            .execute(include_str!("members.sql"), params![name, side])?;
        self.connection.execute_batch("DELETE FROM resolution_candidates; INSERT INTO resolution_candidates SELECT id FROM resolution_members;")?;
        if barred || side == "ambiguous" {
            self.candidate_only = true;
            self.reason = Some(
                "Ruby lookup can change through factories, mixins, refinements or dynamic method mutation",
            );
        }
        self.rule = "ruby_explicit_owner_dispatch_side";
        self.ruby_side_checked = true;
        Ok(())
    }

    pub(in crate::application::analysis::resolve) fn ruby_implicit(
        &mut self,
        c: &FileContext,
        scope: u32,
        name: &str,
    ) -> Result<bool> {
        self.connection.execute("INSERT OR IGNORE INTO resolution_candidates WITH RECURSIVE parents(id,depth) AS (
            VALUES(?2,0) UNION ALL SELECT s.parent,p.depth+1 FROM parents p JOIN scopes s ON s.file_id=?1 AND s.local_id=p.id
            WHERE s.parent IS NOT NULL AND p.depth<1024)
          SELECT s.id FROM parents p JOIN symbols s ON s.file_id=?1 AND s.local_id=p.id
          WHERE s.kind IN ('class','module') ORDER BY p.depth LIMIT 1",params![c.id,scope])?;
        self.ruby_members(name)?;
        let found: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM resolution_candidates)",
            [],
            |r| r.get(0),
        )?;
        if !found {
            self.ruby_side_checked = false;
        }
        Ok(found)
    }

    pub(in crate::application::analysis::resolve) fn ruby_dispatch(&mut self) -> Result<()> {
        if self.language != Language::Ruby {
            return Ok(());
        }
        if matches!(self.relation, "inherits" | "mixes_in" | "uses_type") {
            self.connection.execute("DELETE FROM resolution_candidates WHERE id NOT IN (SELECT id FROM symbols WHERE kind IN ('class','module','type'))",[])?;
            return Ok(());
        }
        if !matches!(self.relation, "calls" | "references") {
            return Ok(());
        }
        let side = self.ruby_side()?;
        let barred = self.ruby_barred()?;
        if !self.ruby_side_checked {
            self.connection.execute("DELETE FROM resolution_candidates WHERE id IN (
            SELECT s.id FROM symbols s JOIN bindings b ON b.file_id=s.file_id AND b.target_local=s.local_id
            WHERE b.kind='ruby_dispatch' AND ?1<>'ambiguous' AND b.module_name NOT IN (?1,'ambiguous'))",[&side])?;
        }
        let uncertain:bool=self.connection.query_row("SELECT EXISTS(SELECT 1 FROM resolution_candidates c JOIN symbols s ON s.id=c.id JOIN bindings b ON b.file_id=s.file_id AND b.target_local=s.local_id WHERE b.kind='ruby_dispatch' AND b.module_name='ambiguous')",[],|r|r.get(0))?;
        if barred || uncertain || side == "ambiguous" {
            self.candidate_only = true;
            self.reason = Some(
                "Ruby lookup can change through factories, mixins, refinements or dynamic method mutation",
            );
        }
        Ok(())
    }

    pub(in crate::application::analysis::resolve) fn ruby_constructor(&mut self) -> Result<()> {
        if self.language != Language::Ruby {
            return Ok(());
        }
        self.connection
            .execute("DELETE FROM resolution_members", [])?;
        self.connection.execute("INSERT OR IGNORE INTO resolution_members WITH RECURSIVE owners(id) AS (
            SELECT id FROM resolution_candidates UNION SELECT r.target_id FROM owners o JOIN relationships r ON r.source_id=o.id JOIN facts f ON f.id=r.fact_id
            WHERE f.relation='inherits' AND r.resolution='resolved' AND NOT EXISTS(SELECT 1 FROM symbols m JOIN bindings b ON b.file_id=m.file_id AND b.target_local=m.local_id WHERE m.owner_id=o.id AND m.name='new' AND b.kind='ruby_dispatch' AND b.module_name IN ('singleton','ambiguous')))
          SELECT m.id FROM owners o JOIN symbols m ON m.owner_id=o.id JOIN bindings b ON b.file_id=m.file_id AND b.target_local=m.local_id
          WHERE m.name='new' AND b.kind='ruby_dispatch' AND b.module_name IN ('singleton','ambiguous')",[])?;
        let overridden: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM resolution_members)",
            [],
            |r| r.get(0),
        )?;
        if overridden {
            self.connection.execute_batch("DELETE FROM resolution_candidates; INSERT INTO resolution_candidates SELECT id FROM resolution_members;")?;
        }
        Ok(())
    }
}
