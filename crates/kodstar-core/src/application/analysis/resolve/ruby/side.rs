// Purpose: 한 Ruby fact의 instance·singleton 방향을 한 번 판정해 Lookup에 보관.
// Pattern: Simple Module.
// Usage: ruby_members와 ruby_dispatch가 같은 fact 수명에서 조회.
// Related: crates/kodstar-core/src/application/analysis/resolve/ruby/mod.rs, crates/kodstar-core/src/application/analysis/resolve/facts.rs.

use crate::application::analysis::resolve::Lookup;
use crate::domain::diagnostics::Result;

#[derive(Clone, Copy, PartialEq)]
pub(in crate::application::analysis::resolve) enum Side {
    Instance,
    Singleton,
    Ambiguous,
}

impl Side {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Instance => "instance",
            Self::Singleton => "singleton",
            Self::Ambiguous => "ambiguous",
        }
    }
}

impl Lookup<'_, '_> {
    pub(super) fn ruby_side(&mut self) -> Result<Side> {
        if let Some(side) = self.ruby_side {
            return Ok(side);
        }
        let Some(fact) = self.fact else {
            return Ok(Side::Ambiguous);
        };
        let (expression, source): (String, String) = self.connection.cached_row(
            "SELECT f.expression,COALESCE(b.module_name,CASE WHEN s.kind IN ('class','module') THEN 'singleton' ELSE 'instance' END)
            FROM facts f JOIN symbols s ON s.id=f.source_id
            LEFT JOIN bindings b ON b.file_id=s.file_id AND b.target_local=s.local_id
              AND b.kind_id=(SELECT id FROM atoms WHERE value='ruby_dispatch') WHERE f.id=?1",
            [fact], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let source = match source.as_str() {
            "instance" => Side::Instance,
            "singleton" => Side::Singleton,
            _ => Side::Ambiguous,
        };
        let side = if expression.starts_with("@result:") {
            Side::Instance
        } else if expression.starts_with("@constructor:") {
            Side::Singleton
        } else if let Some((receiver, _)) = expression.rsplit_once('.') {
            if receiver == "self" {
                source
            } else if receiver.starts_with("::")
                || receiver.chars().next().is_some_and(char::is_uppercase)
            {
                Side::Singleton
            } else {
                Side::Instance
            }
        } else {
            source
        };
        // The fact and its original source are immutable during this Lookup.
        // Barrier checks depend on mutable candidates and deliberately remain separate.
        self.ruby_side = Some(side);
        Ok(side)
    }
}
