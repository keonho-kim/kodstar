-- Purpose: PHP class·trait·parent 우선순위·별칭·제외·메서드 접근 제한.
-- Pattern: SQLite Recursive Query.
-- Usage: php_members.rs가 이름·이름 공간·사용 사실을 바인딩.
-- Related: crates/kodstar-core/src/application/analysis/resolve/php/members.rs.

INSERT OR IGNORE INTO resolution_members
WITH RECURSIVE
matching(id,owner,visibility,adapted) AS (
  SELECT s.id,s.owner_id,COALESCE(v.module_name,'public'),0 FROM symbols s
    LEFT JOIN bindings v ON v.file_id=s.file_id AND v.target_local=s.local_id AND v.kind='php_visibility'
  WHERE s.language='php'
    AND (?2='function' AND s.kind IN ('method','function') AND s.name=?1 COLLATE NOCASE
      OR ?2<>'function' AND s.kind='field' AND s.name=?1)
  UNION
  SELECT id,owner,visibility,1 FROM php_aliases WHERE ?2='function' AND name=?1 COLLATE NOCASE
),
owners(id,composition) AS (
  SELECT id,id FROM resolution_candidates
  UNION
  SELECT r.target_id,CASE WHEN f.relation='inherits' THEN r.target_id ELSE o.composition END
    FROM owners o JOIN relationships r ON r.source_id=o.id JOIN facts f ON f.id=r.fact_id
  WHERE r.resolution='resolved' AND f.relation IN ('inherits','mixes_in')
    AND NOT EXISTS(SELECT 1 FROM matching m WHERE m.owner=o.id AND (m.adapted OR NOT EXISTS(SELECT 1 FROM php_excluded x WHERE x.owner=o.composition AND x.id=m.id)))
    AND (f.relation='mixes_in' OR NOT EXISTS(SELECT 1 FROM php_traits t JOIN matching m ON m.owner=t.trait WHERE t.owner=o.id AND (m.adapted OR NOT EXISTS(SELECT 1 FROM php_excluded x WHERE x.owner=o.composition AND x.id=m.id))))
),
scope_parents(file,id,depth) AS (
  SELECT file_id,scope_id,0 FROM facts WHERE id=?3
  UNION ALL
  SELECT s.file_id,s.parent,p.depth+1 FROM scope_parents p JOIN scopes s ON s.file_id=p.file AND s.local_id=p.id
    WHERE s.parent IS NOT NULL AND p.depth<1024
),
caller(id) AS (
  SELECT s.id FROM scope_parents p JOIN symbols s ON s.file_id=p.file AND s.local_id=p.id
    WHERE s.kind IN ('class','trait','enum') ORDER BY p.depth LIMIT 1
),
ancestors(root,id) AS (
  SELECT id,id FROM caller UNION SELECT composition,composition FROM owners
  UNION
  SELECT a.root,r.target_id FROM ancestors a JOIN relationships r ON r.source_id=a.id JOIN facts f ON f.id=r.fact_id
    WHERE f.relation='inherits' AND r.resolution='resolved'
)
SELECT m.id FROM owners o JOIN matching m ON m.owner=o.id
WHERE (m.adapted OR NOT EXISTS(SELECT 1 FROM php_excluded x WHERE x.owner=o.composition AND x.id=m.id))
  AND (m.visibility='public'
    OR m.visibility='private' AND o.composition IN (SELECT id FROM caller)
    OR m.visibility='protected' AND EXISTS(SELECT 1 FROM caller c WHERE c.id=o.composition
      OR EXISTS(SELECT 1 FROM ancestors a WHERE a.root=c.id AND a.id=o.composition OR a.root=o.composition AND a.id=c.id)));
