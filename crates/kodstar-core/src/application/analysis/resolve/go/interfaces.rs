// Purpose: Go method set의 구조적 구현·호출 후보를 불확실성과 함께 저장.
// Pattern: Simple Module.
// Usage: 기본 관계 해석 이후 같은 세대 transaction에서 실행.
// Related: crates/kodstar-core/src/application/analysis/resolve/go/method_sets.sql, crates/kodstar-core/src/application/analysis/resolve/mod.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::infrastructure::index::cached::Sql as Connection;
use rusqlite::params;

const REASON: &str = "matching Go method names and arities; signature types, pointer method sets and runtime dispatch are not proven";

pub(in crate::application::analysis::resolve) fn run(
    connection: &Connection,
    cancel: &Cancellation,
) -> Result<()> {
    connection.execute_batch(include_str!("method_sets.sql"))?;
    let mut statement = connection.cached_prepare(
        "SELECT concrete.id,contract.id FROM symbols concrete JOIN symbols contract
         WHERE concrete.language_id=(SELECT id FROM atoms WHERE value='go') AND concrete.kind_id IN (SELECT id FROM atoms WHERE value IN ('struct','type'))
           AND contract.language_id=(SELECT id FROM atoms WHERE value='go') AND contract.kind_id=(SELECT id FROM atoms WHERE value='interface')
           AND EXISTS(SELECT 1 FROM go_methods m WHERE m.root=contract.id)
           AND NOT EXISTS(SELECT 1 FROM go_conflicts c WHERE c.root=contract.id)
           AND NOT EXISTS(SELECT 1 FROM go_ancestors a JOIN bindings b
             ON b.file_id=(SELECT file_id FROM symbols WHERE id=a.id)
             AND b.target_local=(SELECT local_id FROM symbols WHERE id=a.id)
             WHERE a.root=contract.id AND b.kind_id=(SELECT id FROM atoms WHERE value='go_constraint'))
           AND NOT EXISTS(SELECT 1 FROM go_methods required JOIN symbols declaration ON declaration.id=required.id
             JOIN files df ON df.id=declaration.file_id WHERE required.root=contract.id AND NOT EXISTS(
               SELECT 1 FROM go_methods supplied JOIN symbols implementation ON implementation.id=supplied.id
               JOIN files sf ON sf.id=implementation.file_id
               WHERE supplied.root=concrete.id AND supplied.name=required.name AND supplied.arity=required.arity
                 AND (declaration.exported=1 OR (df.package_id=sf.package_id AND df.module_id=sf.module_id AND df.declared_module_id=sf.declared_module_id))
             )) ORDER BY concrete.id,contract.id",
    )?;
    let records = statement.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    for record in records {
        cancel.check()?;
        let (source, target) = record?;
        let fact:i64=connection.cached_row(
            "INSERT INTO facts(id,file_id,source_id,scope_id,expression,relation,language,condition,type_only,start_byte,end_byte,line,column_byte,end_line,end_column_byte)
             SELECT (SELECT COALESCE(MAX(id),0)+1 FROM facts_rows),file_id,id,scope_id,'@go-interface:'||(SELECT logical_key FROM symbols WHERE id=?2),'implements',language,condition,0,start_byte,end_byte,line,column_byte,end_line,end_column_byte FROM symbols WHERE id=?1 RETURNING id",params![source,target],|r|r.get(0))?;
        connection.cached_execute("INSERT INTO relationships(fact_id,source_id,target_id,resolution,rule,reason) VALUES(?1,?2,?3,'candidate','go_method_set',?4)",params![fact,source,target,REASON])?;
    }
    Ok(())
}

pub(in crate::application::analysis::resolve) fn dispatch(
    connection: &Connection,
    cancel: &Cancellation,
    changed_only: bool,
) -> Result<()> {
    let mut statement=connection.cached_prepare(
        "SELECT DISTINCT call.fact_id,call.source_id,implementation.id
         FROM relation_texts rule
         CROSS JOIN relationship_rows implements ON implements.rule_id=rule.id
         JOIN go_methods required ON required.root=implements.target_id
         JOIN symbols declaration ON declaration.id=required.id
         JOIN relationships call ON call.target_id=declaration.id
         JOIN facts f ON f.id=call.fact_id
         JOIN go_methods implementation ON implementation.root=implements.source_id AND implementation.name=required.name
         WHERE rule.value='go_method_set' AND f.relation_id=(SELECT id FROM atoms WHERE value='calls')
           AND call.resolution='resolved' AND declaration.language_id=(SELECT id FROM atoms WHERE value='go')
           AND (?1=0 OR f.file_id IN (SELECT id FROM changed_files) OR f.id IN (SELECT id FROM affected_facts))
         ORDER BY call.fact_id,implementation.id")?;
    let records = statement.query_map([changed_only], |r| {
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
            "INSERT INTO facts(id,file_id,source_id,scope_id,expression,relation,language,condition,type_only,start_byte,end_byte,line,column_byte,end_line,end_column_byte)
             SELECT (SELECT COALESCE(MAX(id),0)+1 FROM facts_rows),file_id,?2,scope_id,expression,'dispatch_candidate',language,condition,type_only,start_byte,end_byte,line,column_byte,end_line,end_column_byte FROM facts WHERE id=?1 RETURNING id",params![origin,source],|r|r.get(0))?;
        connection.cached_execute("INSERT INTO relationships(fact_id,source_id,target_id,resolution,rule,reason) VALUES(?1,?2,?3,'candidate','go_method_set_dispatch',?4)",params![fact,source,target,REASON])?;
    }
    Ok(())
}

pub(in crate::application::analysis::resolve) fn refresh_calls(
    connection: &Connection,
    cancel: &Cancellation,
) -> Result<()> {
    connection.cached_execute(
        "UPDATE facts SET
        start_byte=(SELECT start_byte FROM symbols WHERE id=facts.source_id),
        end_byte=(SELECT end_byte FROM symbols WHERE id=facts.source_id),
        line=(SELECT line FROM symbols WHERE id=facts.source_id),
        column_byte=(SELECT column_byte FROM symbols WHERE id=facts.source_id),
        end_line=(SELECT end_line FROM symbols WHERE id=facts.source_id),
        end_column_byte=(SELECT end_column_byte FROM symbols WHERE id=facts.source_id)
        WHERE language='go' AND relation='implements' AND expression LIKE '@go-interface:%'
          AND file_id IN (SELECT id FROM changed_files)",
        [],
    )?;
    // Package functions cannot dispatch through an interface method set. Check changed
    // call facts before constructing the project-wide temporary method tables.
    let methods:bool=connection.cached_row(
        "SELECT EXISTS(SELECT 1 FROM facts f
         CROSS JOIN relationships r ON r.fact_id=f.id JOIN symbols s ON s.id=r.target_id JOIN symbols owner ON owner.id=s.owner_id
         WHERE (f.file_id IN (SELECT id FROM changed_files) OR f.id IN (SELECT id FROM affected_facts))
           AND f.relation_id=(SELECT id FROM atoms WHERE value='calls') AND r.resolution='resolved' AND s.language='go'
           AND owner.kind IN ('struct','interface','type') AND s.kind IN ('method','function'))",[],|r|r.get(0))?;
    if !methods {
        return Ok(());
    }
    connection.execute_batch(include_str!("method_sets.sql"))?;
    dispatch(connection, cancel, true)
}
