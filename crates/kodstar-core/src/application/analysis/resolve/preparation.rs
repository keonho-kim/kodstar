// Purpose: 실제 정의와 hint에 필요한 전역 해석 준비만 선택.
// Pattern: Coordinator.
// Usage: resolver가 staging과 고정 statement를 준비한 후 호출.
// Related: crates/kodstar-core/src/application/analysis/resolve/mod.rs, crates/kodstar-core/src/application/analysis/resolve/identities.rs.

use super::{cxx, identities, owners, prepared::Prepared};
use crate::domain::{cancellation::Cancellation, diagnostics::Result};
use crate::infrastructure::index::cached::Sql;

pub(super) fn run<'conn>(
    c: &Sql<'conn>,
    cancel: &Cancellation,
    prepared: &Prepared<'conn>,
    all: bool,
) -> Result<()> {
    if all {
        identities::run(c, cancel)?;
    }
    super::namespaces::prepare_paths(c)?;
    let cxx: bool = c.cached_row("SELECT EXISTS(SELECT 1 FROM symbols_rows WHERE language IN (SELECT id FROM atoms WHERE value IN ('c','cpp')))", [], |r| r.get(0))?;
    if cxx {
        cxx::reset(c, cancel)?;
    }
    if !all {
        let added: bool = c.cached_row("SELECT EXISTS(SELECT 1 FROM append_symbols)", [], |r| {
            r.get(0)
        })?;
        if added {
            identities::added(c, cancel)?;
            let owners: bool = c.cached_row("SELECT EXISTS(SELECT 1 FROM bindings b JOIN symbol_additions s ON s.file_id=b.file_id AND s.local_id=b.target_local WHERE b.kind='owner_hint')", [], |r| r.get(0))?;
            if owners {
                owners::run(c, cancel, prepared, true)?;
                identities::rebuild_added(c)?;
            }
        }
        return Ok(());
    }
    if cxx {
        cxx::types::run(c, cancel)?;
        identities::rebuild(c)?;
    }
    let owners: bool = c.cached_row("SELECT EXISTS(SELECT 1 FROM bindings_rows WHERE kind IN (SELECT id FROM atoms WHERE value='owner_hint'))", [], |r| r.get(0))?;
    if owners {
        owners::run(c, cancel, prepared, false)?;
    }
    if cxx {
        cxx::declarations::run(c, cancel)?;
        cxx::functions::run(c, cancel)?;
    }
    if cxx || owners {
        identities::rebuild(c)?;
    }
    Ok(())
}
