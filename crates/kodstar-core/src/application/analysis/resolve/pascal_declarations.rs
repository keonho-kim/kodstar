// Purpose: 같은 unit·소유·구문 서명의 Pascal 선언과 단일 정의를 논리적으로 대응.
// Pattern: Simple Module.
// Usage: 명시적 메서드 소유권 해석 뒤 같은 갱신 transaction에서 실행.
// Related: crates/kodstar-core/src/application/analysis/resolve/identities.rs, crates/kodstar-core/src/infrastructure/parse/pascal/signatures.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::infrastructure::index::cached::Sql as Connection;
use rusqlite::params;

pub(super) fn run(c: &Connection, cancel: &Cancellation) -> Result<()> {
    let mut statement=c.cached_prepare("SELECT DISTINCT s.file_id,s.owner_id,s.name_key,s.kind,b.module_name,COALESCE(s.condition,''),f.path,f.package,lower(f.module),lower(o.qualified_name)
      FROM symbols s JOIN files f ON f.id=s.file_id JOIN symbols o ON o.id=s.owner_id
      JOIN bindings b ON b.file_id=s.file_id AND b.target_local=s.local_id AND b.kind='pascal_signature'
      WHERE s.language='pascal' ORDER BY s.file_id,s.owner_id,s.name_key,s.kind,b.module_name")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let file: i64 = row.get(0)?;
        let owner: i64 = row.get(1)?;
        let name: String = row.get(2)?;
        let kind: String = row.get(3)?;
        let signature: String = row.get(4)?;
        let condition: String = row.get(5)?;
        let (declarations,definitions):(u64,u64)=c.cached_row("SELECT COALESCE(SUM(b.imported_name='declaration'),0),COALESCE(SUM(b.imported_name='definition'),0)
          FROM symbols s JOIN bindings b ON b.file_id=s.file_id AND b.target_local=s.local_id AND b.kind='pascal_signature'
          WHERE s.file_id=?1 AND s.owner_id=?2 AND s.name_key=?3 AND s.kind=?4 AND b.module_name=?5 AND COALESCE(s.condition,'')=?6",
          params![file,owner,name,kind,signature,condition],|r|Ok((r.get(0)?,r.get(1)?)))?;
        if declarations > 1 || definitions > 1 {
            continue;
        }
        let mut hash = blake3::Hasher::new();
        hash.update(b"pascal-declaration");
        let path: Vec<u8> = row.get(6)?;
        hash.update(&(path.len() as u64).to_le_bytes());
        hash.update(&path);
        for part in [
            row.get::<_, String>(7)?,
            row.get(8)?,
            row.get(9)?,
            name.clone(),
            kind.clone(),
            signature.clone(),
            condition.clone(),
        ] {
            hash.update(&(part.len() as u64).to_le_bytes());
            hash.update(part.as_bytes());
        }
        c.cached_execute("UPDATE symbols SET logical_key=?7 WHERE id IN (
          SELECT s.id FROM symbols s JOIN bindings b ON b.file_id=s.file_id AND b.target_local=s.local_id AND b.kind='pascal_signature'
          WHERE s.file_id=?1 AND s.owner_id=?2 AND s.name_key=?3 AND s.kind=?4 AND b.module_name=?5 AND COALESCE(s.condition,'')=?6)",
          params![file,owner,name,kind,signature,condition,hash.finalize().to_hex().to_string()])?;
    }
    Ok(())
}
