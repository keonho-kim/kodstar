// Purpose: 소형 인덱스의 일곱 저장 계약을 ID와 무관하게 비교.
// Pattern: Test Snapshot.
// Usage: 증분 최적화 회귀 사례가 동일 입력의 full build와 대조.
// Related: crates/kodstar-core/tests/cases/optimization.rs, scripts/benchmarks/analysis.py.

use kodstar_core::InitReport;
use std::error::Error;

pub fn normalized_index(report: &InitReport) -> Result<Vec<String>, Box<dyn Error>> {
    let connection =
        rusqlite::Connection::open(std::path::Path::new(&report.cache).join("index.sqlite"))?;
    super::postings::register(&connection)?;
    let mut result = Vec::new();
    for (name, sql) in [
        ("symbols", "
            SELECT hex(f.path),s.logical_key,s.name,s.qualified_name,s.kind,s.language,
                   s.signature,s.condition,o.logical_key,s.start_byte,s.end_byte,
                   s.scope_id,f.module,f.package,s.line,s.column_byte,s.end_line,s.end_column_byte,s.exported,
                   s.syntax_key,s.syntax_qualified_name,s.syntax_kind
            FROM symbols s JOIN files f ON f.id=s.file_id LEFT JOIN symbols o ON o.id=s.owner_id
            ORDER BY 1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22
        "),
        ("relations", "
            SELECT hex(f.path),s.logical_key,t.logical_key,r.external_key,u.relation,
                   u.expression,u.start_byte,u.end_byte,u.condition,u.type_only,
                   r.resolution,r.rule,r.reason
            FROM relationships r JOIN facts u ON u.id=r.fact_id
            JOIN files f ON f.id=u.file_id JOIN symbols s ON s.id=r.source_id
            LEFT JOIN symbols t ON t.id=r.target_id
            ORDER BY 1,2,3,4,5,6,7,8,9,10,11,12,13
        "),
        ("inputs", "
            SELECT hex(path),CASE WHEN typeof(content_hash)='blob' THEN lower(hex(content_hash)) ELSE content_hash END,language,module,declared_module,package,status
            FROM files ORDER BY path
        "),
        ("scopes", "
            SELECT hex(f.path),s.local_id,s.parent,s.start_byte,s.end_byte
            FROM scopes s JOIN files f ON f.id=s.file_id ORDER BY 1,2,3,4,5
        "),
        ("bindings", "
            SELECT hex(f.path),b.scope_id,b.name,b.name_key,b.target_local,b.module_name,
                   b.imported_name,b.type_name,b.kind,b.start_byte,b.end_byte,b.namespace
            FROM bindings b JOIN files f ON f.id=b.file_id ORDER BY 1,2,3,4,5,6,7,8,9,10,11,12
        "),
        ("identities", "
            SELECT s.syntax_key,c.logical_key FROM symbol_identities i
            JOIN symbols s ON s.id=i.symbol_id JOIN symbols c ON c.id=i.canonical_id
            ORDER BY 1,2
        "),
        ("lookups", include_str!("../../../../scripts/benchmarks/lookups.sql")),
    ] {
        result.push(name.into());
        let mut statement = connection.prepare(sql)?;
        let columns = statement.column_count();
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let mut values = Vec::with_capacity(columns);
            for index in 0..columns {
                values.push(format!("{:?}", row.get_ref(index)?));
            }
            result.push(format!("{values:?}"));
        }
    }
    Ok(result)
}
