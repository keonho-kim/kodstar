// Purpose: Go method set의 구조적 구현·호출 후보를 불확실성과 함께 저장.
// Pattern: Simple Module.
// Usage: 기본 관계 해석 이후 같은 세대 transaction에서 실행.
// Related: crates/kodstar-core/src/application/analysis/resolve/go/method_sets.sql, crates/kodstar-core/src/application/analysis/resolve/mod.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use rusqlite::{Connection, params};

const REASON: &str = "matching Go method names and arities; signature types, pointer method sets and runtime dispatch are not proven";

pub(in crate::application::analysis::resolve) fn run(
    connection: &Connection,
    cancel: &Cancellation,
) -> Result<()> {
    connection.execute_batch(include_str!("method_sets.sql"))?;
    let mut statement = connection.prepare(
        "SELECT concrete.id,contract.id FROM symbols concrete JOIN symbols contract
         WHERE concrete.language='go' AND concrete.kind IN ('struct','type')
           AND contract.language='go' AND contract.kind='interface'
           AND EXISTS(SELECT 1 FROM go_methods m WHERE m.root=contract.id)
           AND NOT EXISTS(SELECT 1 FROM go_conflicts c WHERE c.root=contract.id)
           AND NOT EXISTS(SELECT 1 FROM go_ancestors a JOIN bindings b
             ON b.file_id=(SELECT file_id FROM symbols WHERE id=a.id)
             AND b.target_local=(SELECT local_id FROM symbols WHERE id=a.id)
             WHERE a.root=contract.id AND b.kind='go_constraint')
           AND NOT EXISTS(SELECT 1 FROM go_methods required JOIN symbols declaration ON declaration.id=required.id
             JOIN files df ON df.id=declaration.file_id WHERE required.root=contract.id AND NOT EXISTS(
               SELECT 1 FROM go_methods supplied JOIN symbols implementation ON implementation.id=supplied.id
               JOIN files sf ON sf.id=implementation.file_id
               WHERE supplied.root=concrete.id AND supplied.name=required.name AND supplied.arity=required.arity
                 AND (declaration.exported=1 OR (df.package=sf.package AND df.module=sf.module AND df.declared_module=sf.declared_module))
             )) ORDER BY concrete.id,contract.id",
    )?;
    let records = statement.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    for record in records {
        cancel.check()?;
        let (source, target) = record?;
        let fact:i64=connection.query_row(
            "INSERT INTO facts(file_id,source_id,scope_id,expression,relation,language,condition,type_only,start_byte,end_byte,line,column_byte,end_line,end_column_byte)
             SELECT file_id,id,scope_id,'@go-interface:'||(SELECT logical_key FROM symbols WHERE id=?2),'implements',language,condition,0,start_byte,end_byte,line,column_byte,end_line,end_column_byte FROM symbols WHERE id=?1 RETURNING id",params![source,target],|r|r.get(0))?;
        connection.execute("INSERT INTO relationships(fact_id,source_id,target_id,resolution,rule,reason) VALUES(?1,?2,?3,'candidate','go_method_set',?4)",params![fact,source,target,REASON])?;
    }
    Ok(())
}

pub(in crate::application::analysis::resolve) fn dispatch(
    connection: &Connection,
    cancel: &Cancellation,
) -> Result<()> {
    let mut statement=connection.prepare(
        "SELECT DISTINCT call.fact_id,call.source_id,implementation.id FROM relationships call
         JOIN facts f ON f.id=call.fact_id JOIN symbols declaration ON declaration.id=call.target_id
         JOIN go_methods required ON required.id=declaration.id
         JOIN relationships implements ON implements.target_id=required.root AND implements.rule='go_method_set'
         JOIN go_methods implementation ON implementation.root=implements.source_id AND implementation.name=required.name
         WHERE f.relation='calls' AND call.resolution='resolved' AND declaration.language='go'
         ORDER BY call.fact_id,implementation.id")?;
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
        let fact:i64=connection.query_row(
            "INSERT INTO facts(file_id,source_id,scope_id,expression,relation,language,condition,type_only,start_byte,end_byte,line,column_byte,end_line,end_column_byte)
             SELECT file_id,?2,scope_id,expression,'dispatch_candidate',language,condition,type_only,start_byte,end_byte,line,column_byte,end_line,end_column_byte FROM facts WHERE id=?1 RETURNING id",params![origin,source],|r|r.get(0))?;
        connection.execute("INSERT INTO relationships(fact_id,source_id,target_id,resolution,rule,reason) VALUES(?1,?2,?3,'candidate','go_method_set_dispatch',?4)",params![fact,source,target,REASON])?;
    }
    Ok(())
}
