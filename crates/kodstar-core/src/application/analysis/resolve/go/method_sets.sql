-- Purpose: Go embedding에서 가까운 메서드와 유일한 승격 후보 계산.
-- Pattern: SQLite Working Set.
-- Usage: go_interfaces가 세대별 기본 해석 뒤 실행.
-- Related: crates/kodstar-core/src/application/analysis/resolve/go/interfaces.rs.

CREATE TEMP TABLE IF NOT EXISTS go_ancestors(root INTEGER,id INTEGER,depth INTEGER,PRIMARY KEY(root,id,depth));
CREATE TEMP TABLE IF NOT EXISTS go_methods(root INTEGER,name INTEGER,id INTEGER,arity INTEGER,depth INTEGER,PRIMARY KEY(root,id));
CREATE TEMP TABLE IF NOT EXISTS go_conflicts(root INTEGER,name INTEGER,PRIMARY KEY(root,name));
DELETE FROM go_ancestors;
DELETE FROM go_methods;
DELETE FROM go_conflicts;

WITH RECURSIVE ancestors(root,id,depth) AS (
 SELECT id,id,0 FROM symbols WHERE language_id=(SELECT id FROM atoms WHERE value='go') AND kind_id IN (SELECT id FROM atoms WHERE value IN ('struct','interface','type'))
 UNION
 SELECT a.root,r.target_id,a.depth+1 FROM ancestors a
 JOIN relationships r ON r.source_id=a.id
 JOIN facts f ON f.id=r.fact_id
 WHERE r.resolution='resolved' AND f.relation_id=(SELECT id FROM atoms WHERE value='mixes_in') AND a.depth<32
) INSERT INTO go_ancestors SELECT * FROM ancestors;

INSERT INTO go_methods
 SELECT a.root,m.name_id,m.id,b.module_name_id,MIN(a.depth) FROM go_ancestors a
 JOIN symbols m ON m.owner_id=a.id AND m.kind_id IN (SELECT id FROM atoms WHERE value IN ('method','function'))
 JOIN bindings b ON b.file_id=m.file_id AND b.target_local=m.local_id AND b.kind_id=(SELECT id FROM atoms WHERE value='go_signature')
 GROUP BY a.root,m.id;

INSERT INTO go_conflicts
 SELECT root,name FROM go_methods GROUP BY root,name
 HAVING COUNT(*)>1;
DELETE FROM go_methods AS m WHERE EXISTS(
 SELECT 1 FROM go_methods nearer WHERE nearer.root=m.root AND nearer.name=m.name AND nearer.depth<m.depth
);
DELETE FROM go_conflicts WHERE (SELECT COUNT(*) FROM go_methods m WHERE m.root=go_conflicts.root AND m.name=go_conflicts.name)<2;
DELETE FROM go_methods AS m WHERE EXISTS(SELECT 1 FROM go_conflicts c WHERE c.root=m.root AND c.name=m.name);
