// Purpose: 명시적 소유·상속 경로의 멤버 조회와 override 우선순위.
// Pattern: Simple Module.
// Usage: lexical·module lookup이 식별한 receiver 후보에 적용.
// Related: crates/kodstar-core/src/application/analysis/resolve/lexical.rs, crates/kodstar-core/src/application/analysis/resolve/dispatch.rs.

use crate::application::analysis::resolve::Lookup;
use crate::domain::diagnostics::Result;

impl Lookup<'_, '_> {
    pub(super) fn members(&mut self, member: &str) -> Result<()> {
        if self.member_depth >= 32 {
            self.reason = Some("member type lookup depth limit");
            self.connection
                .execute("DELETE FROM resolution_candidates", [])?;
            return Ok(());
        }
        self.member_depth += 1;
        let result = self.member_targets(member);
        self.member_depth -= 1;
        result
    }

    fn member_targets(&mut self, member: &str) -> Result<()> {
        let member = if self.language == crate::domain::languages::Language::Rust {
            member.trim_end_matches('!')
        } else {
            member
        };
        let elixir = self.language == crate::domain::languages::Language::Elixir;
        let keyed = matches!(
            self.language,
            crate::domain::languages::Language::Rust
                | crate::domain::languages::Language::Pascal
                | crate::domain::languages::Language::Kotlin
        );
        let key = keyed.then(|| self.language.lookup_key(member));
        let member = key.as_deref().unwrap_or(member);
        let (member, arity) = if elixir {
            member
                .rsplit_once('/')
                .and_then(|(n, a)| a.parse::<u32>().ok().map(|a| (n, Some(a))))
                .unwrap_or((member, None))
        } else {
            (member, None)
        };
        self.field_types()?;
        self.property_types()?;
        self.external_members(member)?;
        if self.rust_macro_members(member)? {
            return Ok(());
        }
        if self.language == crate::domain::languages::Language::Ruby {
            return self.ruby_members(member);
        }
        if self.language == crate::domain::languages::Language::Php {
            return self.php_members(member);
        }
        if self.python_members(member, false)? {
            return Ok(());
        }
        self.rule = "explicit_owner_or_receiver_type";
        self.connection
            .execute("DELETE FROM resolution_members", [])?;
        // UNION deduplicates cycles on disk. A declaration on a nearer type stops that branch.
        self.connection.execute(
            include_str!("members.sql"),
            rusqlite::params![member, keyed],
        )?;
        self.connection.execute(
            "INSERT OR IGNORE INTO resolution_members
            SELECT m.id FROM symbols m JOIN symbols t ON m.qualified_name=t.qualified_name||'::'||?1
            JOIN resolution_candidates c ON c.id=t.id JOIN files mf ON mf.id=m.file_id
            JOIN files tf ON tf.id=t.file_id WHERE mf.language=tf.language AND mf.module=tf.module
            AND mf.package=tf.package AND t.kind<>'file' AND t.language<>'scala'",
            [member],
        )?;
        self.connection.execute_batch("DELETE FROM resolution_candidates; INSERT INTO resolution_candidates SELECT id FROM resolution_members;")?;
        if let Some(arity) = arity {
            self.connection.execute("DELETE FROM resolution_candidates WHERE id IN (SELECT id FROM symbols WHERE language='elixir' AND exported=0)",[])?;
            self.connection.execute(
                "DELETE
                FROM resolution_candidates
                WHERE NOT EXISTS(SELECT 1
                FROM symbols s
                JOIN bindings b ON b.file_id=s.file_id
                AND b.target_local=s.local_id
                WHERE s.id=resolution_candidates.id
                AND b.kind='elixir_function'
                AND CAST(b.imported_name AS INTEGER)<=?1
                AND CAST(b.module_name AS INTEGER)>=?1)",
                [arity],
            )?;
        }
        self.rust_targets()?;
        if self.language == crate::domain::languages::Language::Swift {
            self.connection.execute("DELETE FROM resolution_candidates WHERE id IN (
                SELECT s.id FROM symbols s JOIN files target ON target.id=s.file_id
                JOIN facts fact ON fact.id=?1 JOIN files caller ON caller.id=fact.file_id
                WHERE s.exported=0 AND (target.package<>caller.package OR target.module<>caller.module))",[self.fact])?;
        }
        if self.language == crate::domain::languages::Language::Go {
            self.connection.execute("DELETE FROM resolution_candidates WHERE id IN (
                SELECT s.id FROM symbols s JOIN files target ON target.id=s.file_id
                JOIN facts fact ON fact.id=?1 JOIN files caller ON caller.id=fact.file_id
                WHERE s.exported=0 AND (target.package<>caller.package OR target.module<>caller.module OR target.declared_module<>caller.declared_module))",[self.fact])?;
        }
        if matches!(
            self.language,
            crate::domain::languages::Language::Zig | crate::domain::languages::Language::Pascal
        ) {
            self.connection.execute("DELETE FROM resolution_candidates WHERE id IN (SELECT s.id FROM symbols s WHERE s.exported=0 AND s.file_id<>(SELECT file_id FROM facts WHERE id=?1))",[self.fact])?;
        }
        Ok(())
    }
}
