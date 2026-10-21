// Purpose: 정적 trait 별칭·제외 규칙을 실제 정의와 대응해 세대별 cache 구성.
// Pattern: Simple Module.
// Usage: PHP 멤버 해석의 첫 조회가 제한된 인덱스 사실을 순회.
// Related: crates/kodstar-core/src/application/analysis/resolve/php/members.rs, crates/kodstar-core/src/application/analysis/resolve/php/traits.sql.

use crate::application::analysis::resolve::Lookup;
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use rusqlite::params;

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn php_prepare_traits(&mut self) -> Result<()> {
        let ready: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM php_traits_ready)",
            [],
            |r| r.get(0),
        )?;
        if ready {
            return Ok(());
        }
        self.connection.execute_batch("INSERT
            OR IGNORE INTO php_traits WITH RECURSIVE bases(owner,trait) AS (SELECT r.source_id,r.target_id
            FROM relationships r
            JOIN facts f ON f.id=r.fact_id
            WHERE f.language='php'
            AND f.relation='mixes_in'
            AND r.resolution='resolved'
            UNION
            SELECT b.owner,r.target_id
            FROM bases b
            JOIN relationships r ON r.source_id=b.trait
            JOIN facts f ON f.id=r.fact_id
            WHERE f.relation='mixes_in'
            AND r.resolution='resolved')
            SELECT owner,trait
            FROM bases; INSERT INTO php_trait_receivers
            SELECT id
            FROM resolution_candidates;")?;
        let external = self.external_scope()?;
        let saved = (self.candidate_only, self.reason, self.rule);
        let mut statement=self.connection.prepare("SELECT b.file_id,b.scope_id,b.start_byte,s.id,b.kind,b.name,b.module_name,b.imported_name,b.type_name
            FROM bindings b
            JOIN symbols s ON s.file_id=b.file_id
            AND s.local_id=b.target_local
            WHERE b.kind IN ('php_trait_alias','php_trait_exclusion')
            ORDER BY CASE b.kind WHEN 'php_trait_exclusion' THEN 0 ELSE 1 END,b.id")?;
        let records = statement.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, u32>(1)?,
                r.get::<_, usize>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, Option<String>>(8)?,
            ))
        })?;
        for record in records {
            let (file, scope, position, owner, kind, name, module, imported, visibility) = record?;
            let Some(context) = crate::application::analysis::resolve::context(
                self.connection,
                file,
                Some(Language::Php),
            )?
            else {
                continue;
            };
            self.connection
                .execute("DELETE FROM resolution_candidates", [])?;
            if kind == "php_trait_exclusion" {
                self.type_expression(&context, scope, position, &imported, 0)?;
                let known:bool=self.connection.query_row("SELECT EXISTS(SELECT 1 FROM resolution_candidates c JOIN php_traits t ON t.trait=c.id AND t.owner=?1)",[owner],|r|r.get(0))?;
                if !known {
                    continue;
                }
                self.connection
                    .execute("DELETE FROM resolution_candidates", [])?;
            }
            let explicit = module.is_some();
            if let Some(module) = module {
                self.type_expression(&context, scope, position, &module, 0)?;
                self.connection.execute("DELETE FROM resolution_candidates WHERE id NOT IN (SELECT trait FROM php_traits WHERE owner=?1)",[owner])?;
            } else {
                self.connection.execute("INSERT OR IGNORE INTO resolution_candidates SELECT r.target_id FROM relationships r JOIN facts f ON f.id=r.fact_id WHERE r.source_id=?1 AND f.relation='mixes_in' AND r.resolution='resolved'",[owner])?;
            }
            let method = if kind == "php_trait_exclusion" {
                &name
            } else {
                &imported
            };
            self.php_trait_method_ids(method)?;
            if kind == "php_trait_exclusion" {
                self.connection.execute(
                    "INSERT OR IGNORE INTO php_excluded SELECT ?1,id FROM php_trait_methods",
                    [owner],
                )?;
            } else {
                self.connection.execute(
                    "INSERT
                    OR IGNORE INTO php_aliases
                    SELECT ?1,?2,m.id,COALESCE(?3,v.module_name,'public')
                    FROM php_trait_methods m
                    JOIN symbols s ON s.id=m.id
                    LEFT JOIN bindings v ON v.file_id=s.file_id
                    AND v.target_local=s.local_id
                    AND v.kind='php_visibility'
                    WHERE ?4
                    OR NOT EXISTS(SELECT 1
                    FROM php_excluded x
                    WHERE x.owner=?1
                    AND x.id=m.id)",
                    params![owner, name, visibility, explicit],
                )?;
            }
        }
        self.connection.execute_batch("DELETE FROM resolution_candidates; INSERT INTO resolution_candidates SELECT id FROM php_trait_receivers; DELETE FROM php_trait_receivers; DELETE FROM php_trait_methods; INSERT INTO php_traits_ready VALUES(1);")?;
        self.finish_external_scope(external, false)?;
        (self.candidate_only, self.reason, self.rule) = saved;
        Ok(())
    }

    fn php_trait_method_ids(&self, name: &str) -> Result<()> {
        self.connection
            .execute("DELETE FROM php_trait_methods", [])?;
        self.connection.execute(
            "INSERT
            OR IGNORE INTO php_trait_methods WITH RECURSIVE owners(id) AS (SELECT id
            FROM resolution_candidates
            UNION
            SELECT r.target_id
            FROM owners o
            JOIN relationships r ON r.source_id=o.id
            JOIN facts f ON f.id=r.fact_id
            WHERE f.relation='mixes_in'
            AND r.resolution='resolved'
            AND NOT EXISTS(SELECT 1
            FROM symbols s
            WHERE s.owner_id=o.id
            AND s.kind='method'
            AND s.name=?1 COLLATE NOCASE))
            SELECT s.id
            FROM owners o
            JOIN symbols s ON s.owner_id=o.id
            WHERE s.kind='method'
            AND s.name=?1 COLLATE NOCASE",
            [name],
        )?;
        Ok(())
    }
}
