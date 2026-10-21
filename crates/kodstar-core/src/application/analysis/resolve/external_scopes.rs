// Purpose: lookup scope 번호와 외부 후보의 분기 소유·멤버 경로 정리.
// Pattern: Scoped Resolution State.
// Usage: expression·module 해석과 외부 후보를 생성·정리하는 adapter가 호출.
// Related: crates/kodstar-core/src/application/analysis/resolve/lexical.rs, crates/kodstar-core/src/application/analysis/resolve/publication.rs.

use crate::application::analysis::resolve::Lookup;
use crate::domain::diagnostics::{Error, Result};
use rusqlite::params;

pub(super) struct Frame {
    parent: usize,
    child: usize,
    present: bool,
}

impl Lookup<'_, '_> {
    pub(super) fn allocate_scope(&mut self) -> Result<usize> {
        self.scope_sequence = self
            .scope_sequence
            .checked_add(1)
            .ok_or_else(|| Error::operation("lookup scope overflow"))?;
        Ok(self.scope_sequence)
    }

    pub(super) fn external_scope(&mut self) -> Result<Frame> {
        let child = if self.external_present {
            self.allocate_scope()?
        } else {
            self.external_scope_id
        };
        let frame = Frame {
            parent: self.external_scope_id,
            child,
            present: self.external_present,
        };
        self.external_scope_id = child;
        self.external_present = false;
        Ok(frame)
    }

    pub(super) fn finish_external_scope(&mut self, frame: Frame, keep: bool) -> Result<()> {
        if self.external_scope_id != frame.child {
            return Err(Error::operation("external lookup scope order mismatch"));
        }
        let present = self.external_present;
        if present && frame.parent != frame.child {
            if keep {
                self.connection.execute("INSERT OR IGNORE INTO resolution_externals SELECT ?1,key FROM resolution_externals WHERE scope=?2",params![frame.parent,self.external_scope_id])?;
            }
            self.connection.execute(
                "DELETE FROM resolution_externals WHERE scope=?1",
                [self.external_scope_id],
            )?;
        } else if present && !keep {
            self.connection.execute(
                "DELETE FROM resolution_externals WHERE scope=?1",
                [frame.child],
            )?;
        }
        self.external_scope_id = frame.parent;
        self.external_present = frame.present || keep && present;
        Ok(())
    }

    pub(super) fn external_target(&mut self, key: &str) -> Result<()> {
        self.connection.execute(
            "INSERT OR IGNORE INTO resolution_externals VALUES(?1,?2)",
            params![self.external_scope_id, key],
        )?;
        self.external_present = true;
        Ok(())
    }

    pub(super) fn has_externals(&self) -> bool {
        self.external_present
    }

    pub(super) fn clear_externals(&mut self) -> Result<()> {
        if self.external_present {
            self.connection.execute(
                "DELETE FROM resolution_externals WHERE scope=?1",
                [self.external_scope_id],
            )?;
            self.external_present = false;
        }
        Ok(())
    }

    pub(super) fn external_members(&mut self, name: &str) -> Result<()> {
        if self.has_externals() {
            self.connection.execute(
                "UPDATE resolution_externals SET key=key||'::'||?2 WHERE scope=?1",
                params![self.external_scope_id, name],
            )?;
            self.external_present = true;
        }
        Ok(())
    }
}
