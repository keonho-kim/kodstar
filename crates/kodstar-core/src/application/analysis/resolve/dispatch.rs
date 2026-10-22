// Purpose: 명시적 구현 관계에서 override·dispatch 후보 생성.
// Pattern: Simple Module.
// Usage: 기본 사실 해석이 끝난 같은 갱신 transaction에서 실행.
// Related: crates/kodstar-core/src/application/analysis/resolve/mod.rs, crates/kodstar-core/src/application/analysis/resolve/members.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::infrastructure::index::cached::Sql as Connection;
use rusqlite::params;

pub(super) fn run(connection: &Connection, cancel: &Cancellation) -> Result<()> {
    let available:bool=connection.cached_row(
        "SELECT EXISTS(SELECT 1 FROM facts f CROSS JOIN relationships r ON r.fact_id=f.id WHERE f.relation IN ('inherits','implements') AND r.resolution='resolved')",[],|r|r.get(0))?;
    if !available {
        return Ok(());
    }
    overrides(connection, cancel)?;
    let mut statement = connection.cached_prepare(
        "SELECT DISTINCT f.id,c.source_id,implementation.id
         FROM relationships c JOIN facts f ON f.id=c.fact_id
         JOIN symbols declaration ON declaration.id=c.target_id
         JOIN symbols contract ON contract.id=declaration.owner_id
         JOIN relationships inheritance ON inheritance.target_id=contract.id
         JOIN facts base ON base.id=inheritance.fact_id
         JOIN symbols implementation ON implementation.owner_id=inheritance.source_id
         WHERE f.relation='calls' AND c.resolution='resolved'
           AND contract.kind IN ('interface','trait')
           AND base.relation IN ('inherits','implements') AND inheritance.resolution='resolved'
           AND implementation.name_key=declaration.name_key
           AND implementation.kind IN ('method','function') ORDER BY f.id,implementation.id",
    )?;
    let records = statement.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    for record in records {
        cancel.check()?;
        let (origin, source, target) = record?;
        let fact:i64=connection.cached_row(
            "INSERT INTO facts(file_id,source_id,scope_id,expression,relation,language,condition,type_only,start_byte,end_byte,line,column_byte,end_line,end_column_byte)
             SELECT file_id,?2,scope_id,expression,'dispatch_candidate',language,condition,type_only,start_byte,end_byte,line,column_byte,end_line,end_column_byte FROM facts WHERE id=?1 RETURNING id",params![origin,source],|r|r.get(0))?;
        connection.cached_execute("INSERT INTO relationships(fact_id,source_id,target_id,resolution,rule,reason) VALUES(?1,?2,?3,'candidate','explicit_interface_dispatch','matching member of an explicit implementor; signature and runtime dispatch are not proven')",params![fact,source,target])?;
    }
    Ok(())
}

fn overrides(connection: &Connection, cancel: &Cancellation) -> Result<()> {
    let mut statement = connection.cached_prepare(
        "SELECT DISTINCT child.id,base.id FROM symbols child
         JOIN relationships inheritance ON inheritance.source_id=child.owner_id
         JOIN facts f ON f.id=inheritance.fact_id
         JOIN symbols base ON base.owner_id=inheritance.target_id
         WHERE f.relation IN ('inherits','implements') AND inheritance.resolution='resolved'
           AND child.kind IN ('method','function') AND base.kind IN ('method','function')
           AND child.name_key=base.name_key ORDER BY child.id,base.id",
    )?;
    let records = statement.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    for record in records {
        cancel.check()?;
        let (source, target) = record?;
        let fact:i64=connection.cached_row(
            "INSERT INTO facts(file_id,source_id,scope_id,expression,relation,language,condition,type_only,start_byte,end_byte,line,column_byte,end_line,end_column_byte)
             SELECT file_id,id,scope_id,name,'overrides',language,condition,0,start_byte,end_byte,line,column_byte,end_line,end_column_byte FROM symbols WHERE id=?1 RETURNING id",[source],|r|r.get(0))?;
        connection.cached_execute("INSERT INTO relationships(fact_id,source_id,target_id,resolution,rule,reason) VALUES(?1,?2,?3,'candidate','explicit_base_member','matching member of an explicit base; signature, visibility and override rules are not proven')",params![fact,source,target])?;
    }
    Ok(())
}
