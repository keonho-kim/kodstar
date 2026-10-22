// Purpose: SQLite 작업 집합에서 Python C3 순서와 super 멤버 선택.
// Pattern: Simple Module.
// Usage: Python receiver의 멤버 조회가 호출하며 매 세대 캐시를 비움.
// Related: crates/kodstar-core/src/application/analysis/resolve/members.rs, crates/kodstar-core/src/application/analysis/resolve/lexical.rs.

use crate::application::analysis::resolve::{FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::infrastructure::index::cached::Sql as Connection;
use rusqlite::{OptionalExtension, params};

pub(in crate::application::analysis::resolve) fn reset(c: &Connection) -> Result<()> {
    c.execute_batch("CREATE TEMP TABLE IF NOT EXISTS python_mro_state(id INTEGER PRIMARY KEY,state INTEGER);
        CREATE TEMP TABLE IF NOT EXISTS python_mro(root INTEGER,position INTEGER,id INTEGER,PRIMARY KEY(root,position));
        CREATE TEMP TABLE IF NOT EXISTS python_mro_sequences(root INTEGER,sequence INTEGER,position INTEGER,id INTEGER,PRIMARY KEY(root,sequence,position));
        DELETE FROM python_mro_state; DELETE FROM python_mro; DELETE FROM python_mro_sequences;")?;
    Ok(())
}

impl Lookup<'_, '_> {
    pub(in crate::application::analysis::resolve) fn python_super(
        &mut self,
        context: &FileContext,
        scope: u32,
        position: usize,
        member: &str,
    ) -> Result<()> {
        if context.language != Language::Python
            || self.binding(context, scope, position, "super")?.is_some()
        {
            self.reason = Some("super is shadowed or not a Python builtin");
            return Ok(());
        }
        self.connection.cached_execute("INSERT OR IGNORE INTO resolution_candidates SELECT owner_id FROM symbols WHERE id=(SELECT source_id FROM facts WHERE id=?1) AND owner_id IN (SELECT id FROM symbols WHERE kind='class')",[self.fact])?;
        self.python_members(member, true)?;
        Ok(())
    }

    pub(in crate::application::analysis::resolve) fn python_members(
        &mut self,
        member: &str,
        skip_self: bool,
    ) -> Result<bool> {
        if self.language != Language::Python {
            return Ok(false);
        }
        let (all,python):(u64,u64)=self.connection.cached_row("SELECT COUNT(*),COALESCE(SUM(s.language='python' AND s.kind='class'),0) FROM resolution_candidates c CROSS JOIN symbols s ON s.id=c.id",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
        if all == 0 || all != python {
            return Ok(false);
        }
        self.connection
            .cached_execute("DELETE FROM resolution_members", [])?;
        let mut statement = self
            .connection
            .cached_prepare("SELECT id FROM resolution_candidates ORDER BY id")?;
        let records = statement.query_map([], |r| r.get::<_, i64>(0))?;
        for root in records {
            let root = root?;
            if !skip_self {
                let direct=self.connection.cached_execute("INSERT OR IGNORE INTO resolution_members SELECT id FROM symbols WHERE owner_id=?1 AND name=?2",params![root,member])?;
                if direct > 0 {
                    continue;
                }
            }
            if linearize(self.connection, root, 0)? {
                self.connection.cached_execute(
                    "INSERT
                    OR IGNORE INTO resolution_members
                    SELECT s.id
                    FROM python_mro m
                    JOIN symbols s ON s.owner_id=m.id
                    WHERE m.root=?1
                    AND s.name=?2
                    AND m.position=(SELECT MIN(first.position)
                    FROM python_mro first
                    JOIN symbols found ON found.owner_id=first.id
                    WHERE first.root=?1
                    AND first.position>=?3
                    AND found.name=?2)",
                    params![root, member, usize::from(skip_self)],
                )?;
            } else {
                self.reason =
                    Some("Python C3 order is cyclic, inconsistent, external or exceeds depth 32");
            }
        }
        self.connection.execute_batch("DELETE FROM resolution_candidates; INSERT INTO resolution_candidates SELECT id FROM resolution_members;")?;
        self.rule = if skip_self {
            "python_super_c3"
        } else {
            "python_c3_mro"
        };
        Ok(true)
    }
}

fn linearize(c: &Connection, root: i64, depth: usize) -> Result<bool> {
    let state: Option<u8> = c
        .cached_row(
            "SELECT state FROM python_mro_state WHERE id=?1",
            [root],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(state) = state {
        return Ok(state == 2);
    }
    c.cached_execute("INSERT INTO python_mro_state VALUES(?1,1)", [root])?;
    if depth >= 32 {
        return failed(c, root);
    }
    let unknown:bool=c.cached_row("SELECT EXISTS(SELECT 1 FROM facts f JOIN relationships r ON r.fact_id=f.id WHERE f.source_id=?1 AND f.relation='inherits' AND (r.resolution<>'resolved' OR (r.target_id IS NULL AND COALESCE(r.external_key,'')<>'builtin:python:object')))",[root],|r|r.get(0))?;
    if unknown {
        return failed(c, root);
    }
    c.cached_execute("INSERT INTO python_mro VALUES(?1,0,?1)", [root])?;
    let mut bases=c.cached_prepare("SELECT r.target_id FROM facts f JOIN relationships r ON r.fact_id=f.id WHERE f.source_id=?1 AND f.relation='inherits' AND r.target_id IS NOT NULL AND r.resolution='resolved' ORDER BY f.start_byte,f.id")?;
    let records = bases.query_map([root], |r| r.get::<_, i64>(0))?;
    for (index, base) in records.enumerate() {
        let base = base?;
        if !linearize(c, base, depth + 1)? {
            return failed(c, root);
        }
        c.cached_execute("INSERT INTO python_mro_sequences SELECT ?1,?2,position,id FROM python_mro WHERE root=?3",params![root,index+1,base])?;
        c.cached_execute(
            "INSERT INTO python_mro_sequences VALUES(?1,0,?2,?3)",
            params![root, index, base],
        )?;
    }
    let mut position = 1;
    loop {
        let candidate:Option<i64>=c.cached_row("SELECT head.id FROM python_mro_sequences head
            WHERE head.root=?1 AND head.position=(SELECT MIN(position) FROM python_mro_sequences same WHERE same.root=?1 AND same.sequence=head.sequence)
              AND NOT EXISTS(SELECT 1 FROM python_mro_sequences tail WHERE tail.root=?1 AND tail.id=head.id AND tail.position>(SELECT MIN(position) FROM python_mro_sequences same WHERE same.root=?1 AND same.sequence=tail.sequence))
            ORDER BY head.sequence LIMIT 1",[root],|r|r.get(0)).optional()?;
        let Some(candidate) = candidate else { break };
        c.cached_execute(
            "INSERT INTO python_mro VALUES(?1,?2,?3)",
            params![root, position, candidate],
        )?;
        c.cached_execute(
            "DELETE FROM python_mro_sequences WHERE root=?1 AND id=?2",
            params![root, candidate],
        )?;
        position += 1;
    }
    let remaining: bool = c.cached_row(
        "SELECT EXISTS(SELECT 1 FROM python_mro_sequences WHERE root=?1)",
        [root],
        |r| r.get(0),
    )?;
    if remaining {
        return failed(c, root);
    }
    c.cached_execute("UPDATE python_mro_state SET state=2 WHERE id=?1", [root])?;
    Ok(true)
}

fn failed(c: &Connection, root: i64) -> Result<bool> {
    c.cached_execute("UPDATE python_mro_state SET state=3 WHERE id=?1", [root])?;
    c.cached_execute("DELETE FROM python_mro WHERE root=?1", [root])?;
    c.cached_execute("DELETE FROM python_mro_sequences WHERE root=?1", [root])?;
    Ok(false)
}
