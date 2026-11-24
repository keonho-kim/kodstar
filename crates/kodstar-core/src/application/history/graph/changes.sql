-- Purpose: 논리 사용과 연결 대상의 대응을 확정하고 추가·삭제·변경을 순서대로 반환.
-- Pattern: Comparison Query.
-- Usage: graph/edges.rs가 양쪽 임시 관계 집계 준비 후 실행.
-- Related: crates/kodstar-core/src/application/history/graph/edges.rs.

WITH same_target AS (
    SELECT use_key,endpoint,MIN(CASE WHEN side=0 THEN rowid END) AS before,
                           MIN(CASE WHEN side=1 THEN rowid END) AS after
    FROM edges GROUP BY use_key,endpoint HAVING SUM(side=0)=1 AND SUM(side=1)=1
), single_target AS (
    SELECT use_key,MIN(CASE WHEN side=0 THEN rowid END) AS before,
                  MIN(CASE WHEN side=1 THEN rowid END) AS after
    FROM edges GROUP BY use_key HAVING SUM(side=0)=1 AND SUM(side=1)=1
), pairs(before,after) AS (
    SELECT a.rowid,b.rowid FROM edges a JOIN edges b
      ON b.side=1 AND b.use_key=a.use_key AND b.endpoint=a.endpoint AND b.state=a.state
    WHERE a.side=0
    UNION
    SELECT before,after FROM same_target
    UNION
    SELECT before,after FROM single_target
), changes(kind,before,after) AS (
    SELECT CASE WHEN a.state<>b.state THEN 'resolution_changed'
                WHEN a.endpoint<>b.endpoint THEN 'retargeted' ELSE 'count_changed' END,
           a.rowid,b.rowid
    FROM pairs p JOIN edges a ON a.rowid=p.before JOIN edges b ON b.rowid=p.after
    WHERE a.state<>b.state OR a.endpoint<>b.endpoint OR a.count<>b.count
    UNION ALL
    SELECT 'removed',rowid,NULL FROM edges
    WHERE side=0 AND rowid NOT IN (SELECT before FROM pairs)
    UNION ALL
    SELECT 'added',NULL,rowid FROM edges
    WHERE side=1 AND rowid NOT IN (SELECT after FROM pairs)
)
SELECT c.kind,c.before,c.after FROM changes c JOIN edges e ON e.rowid=COALESCE(c.after,c.before)
ORDER BY e.path,e.start,e.source,e.relation,e.expression,c.kind,e.endpoint,e.state;
