-- Purpose: Compare lookup dependencies using source paths instead of allocated file IDs.
-- Pattern: Verification Query.
-- Usage: benchmark.py and Rust snapshot tests compare equivalent generations and clean stores.
-- Related: scripts/benchmarks/analysis.py, crates/kodstar-core/tests/support/snapshot.rs.

SELECT hex(f.path),s.logical_key,u.scope_id,u.expression,u.relation,u.start_byte,
       u.end_byte,d.kind,
       CASE WHEN d.kind='configuration' THEN
           coalesce('path:'||hex(dependency_file.path),'missing-id:'||d.lookup_key)
         WHEN (d.kind LIKE 'scope:%' OR d.kind='python_star_export')
           AND instr(d.lookup_key,':')>0 THEN
           coalesce('path:'||hex(dependency_file.path),'missing-id:'||d.lookup_key)
               ||substr(d.lookup_key,instr(d.lookup_key,':'))
         ELSE d.lookup_key END,
       d.found
FROM lookup_dependencies d JOIN facts u ON u.id=d.fact_id
JOIN files f ON f.id=u.file_id JOIN symbols s ON s.id=u.source_id
LEFT JOIN files dependency_file ON dependency_file.id=CASE
    WHEN d.kind='configuration' THEN CAST(d.lookup_key AS INTEGER)
    WHEN (d.kind LIKE 'scope:%' OR d.kind='python_star_export')
      AND instr(d.lookup_key,':')>0 THEN
      CAST(substr(d.lookup_key,1,instr(d.lookup_key,':')-1) AS INTEGER)
    ELSE NULL END
ORDER BY 1,2,3,4,5,6,7,8,9,10
