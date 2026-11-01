-- Purpose: Ruby instance/singleton 방향을 유지한 상속·mixin 후보 탐색.
-- Pattern: SQLite Recursive Query.
-- Usage: ruby/mod.rs가 실제 receiver 후보와 이름·dispatch side를 전달.
-- Related: crates/kodstar-core/src/application/analysis/resolve/ruby/mod.rs.

INSERT OR IGNORE INTO resolution_members
WITH RECURSIVE matching(id,owner,side) AS (
  SELECT s.id,s.owner_id,COALESCE(b.module_name,'any') FROM symbols s
  LEFT JOIN bindings b ON b.file_id=s.file_id AND b.target_local=s.local_id
    AND b.kind_id=(SELECT id FROM atoms WHERE value='ruby_dispatch')
  WHERE s.language_id=(SELECT id FROM atoms WHERE value='ruby')
    AND s.name_key_id=(SELECT id FROM atoms WHERE value=?1)
), owners(id,side) AS (
  SELECT id,?2 FROM resolution_candidates
  UNION
  SELECT r.target_id,CASE WHEN f.relation='mixes_in' THEN 'instance' ELSE o.side END
  FROM owners o JOIN relationships r ON r.source_id=o.id JOIN facts f ON f.id=r.fact_id
  WHERE r.resolution='resolved' AND f.relation IN ('inherits','mixes_in')
    AND (f.relation='inherits'
      OR o.side IN ('singleton','ambiguous') AND f.expression LIKE '@ruby-mixin:extend#%'
      OR o.side IN ('instance','ambiguous') AND (f.expression LIKE '@ruby-mixin:include#%' OR f.expression LIKE '@ruby-mixin:prepend#%'))
    AND (f.expression LIKE '@ruby-mixin:prepend#%' OR NOT EXISTS(SELECT 1 FROM matching m WHERE m.owner=o.id AND (o.side='ambiguous' OR m.side IN (o.side,'any','ambiguous'))))
)
SELECT m.id FROM owners o JOIN matching m ON m.owner=o.id
WHERE o.side='ambiguous' OR m.side IN (o.side,'any','ambiguous');
