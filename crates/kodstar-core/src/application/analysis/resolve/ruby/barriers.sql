-- Purpose: 현재 Ruby 후보와 상속 소유자·파일에 해당하는 조회 제한 표식 탐색.
-- Pattern: SQLite Recursive Query.
-- Usage: ruby/barriers.rs가 scoped 표식이 있는 작업에서 현재 fact를 전달.
-- Related: crates/kodstar-core/src/application/analysis/resolve/ruby/barriers.rs.

WITH RECURSIVE owners(id) AS (
    SELECT CASE WHEN s.kind_id IN (SELECT id FROM atoms WHERE value IN ('method','function')) THEN s.owner_id ELSE s.id END
    FROM resolution_candidates c CROSS JOIN symbols s ON s.id=c.id
    UNION
    SELECT r.target_id FROM owners o JOIN relationships r ON r.source_id=o.id JOIN facts f ON f.id=r.fact_id
    WHERE f.relation_id IN (SELECT id FROM atoms WHERE value IN ('inherits','mixes_in')) AND r.resolution='resolved'
), files(id) AS (
    SELECT s.file_id FROM owners o CROSS JOIN symbols s ON s.id=o.id
    UNION SELECT file_id FROM facts WHERE id=?1
), affected(id) AS (
    SELECT id FROM owners
    UNION
    SELECT s.id FROM files f CROSS JOIN symbols s ON s.file_id=f.id
    WHERE s.kind_id=(SELECT id FROM atoms WHERE value='file')
)
SELECT EXISTS(
    SELECT 1 FROM affected a CROSS JOIN symbols s ON s.id=a.id
    JOIN bindings b ON b.file_id=s.file_id AND b.target_local=s.local_id
    WHERE b.kind_id=(SELECT id FROM atoms WHERE value='ruby_lookup_barrier')
);
