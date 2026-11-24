// Purpose: 메모리·디스크 병합 결과의 동일성과 취소 후 자원 수명 검증.
// Pattern: Unit Test.
// Usage: conditional reducer 테스트가 작은 예산으로 spill을 강제.
// Related: crates/kodstar-core/src/infrastructure/parse/conditional/facts.rs.

use super::*;
use crate::domain::{relations::RelationKind, source::SourceSpan, symbols::SymbolKind};

fn reduce(limit: usize) -> Result<Vec<Fact>> {
    let cancel = Cancellation::default();
    let mut output = Vec::new();
    let mut emit = |fact| {
        output.push(fact);
        Ok(())
    };
    let mut merger = Merger::new(limit, &cancel, &mut emit);
    for _ in 0..3 {
        merger.begin()?;
        for local in 1..=32 {
            let span = SourceSpan {
                start_byte: local as usize,
                end_byte: local as usize + 1,
                line: 1,
                column: local as usize + 1,
                end_line: 1,
                end_column: local as usize + 2,
            };
            merger.push(Fact::Scope(Scope {
                local_id: local,
                parent: Some(0),
                span,
            }))?;
            merger.push(Fact::Definition(Definition {
                local_id: local,
                owner: Some(0),
                scope: 0,
                name: format!("function_{local}"),
                qualified_name: format!("function_{local}"),
                kind: SymbolKind::Function,
                signature: "()".into(),
                condition: None,
                span,
                exported: true,
            }))?;
            merger.push(Fact::Use(crate::domain::facts::SyntaxUse {
                source: local,
                scope: local,
                expression: "target".into(),
                relation: RelationKind::Calls,
                span,
                condition: None,
                type_only: false,
            }))?;
        }
    }
    merger.finish()?;
    drop(merger);
    Ok(output)
}

#[test]
fn disk_spill_keeps_exact_facts_ids_and_duplicate_counts() -> Result<()> {
    let memory = reduce(1024 * 1024)?;
    let disk = reduce(4096)?;
    assert_eq!(memory, disk);
    assert_eq!(disk.len(), 96);
    Ok(())
}

#[test]
fn cancelled_spill_stops_before_writing_and_closes_its_store() -> Result<()> {
    let cancel = Cancellation::default();
    let spill = Spill::new(4096)?;
    spill.insert(2, &"first", 0, &cancel)?;
    cancel.cancel();
    assert_eq!(
        spill.insert(2, &"second", 0, &cancel).unwrap_err().kind,
        ErrorKind::Cancelled
    );
    drop(spill);
    Ok(())
}
