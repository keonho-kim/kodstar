-- Purpose: Lisp export·명시적 import·use·shadow를 적용한 제한된 package 탐색.
-- Pattern: SQLite Recursive Query.
-- Usage: Lisp lookup이 package·이름·이름 공간을 바인딩.
-- Related: crates/kodstar-core/src/application/analysis/resolve/lisp/mod.rs.

INSERT OR IGNORE INTO resolution_candidates
WITH RECURSIVE
search(package,external_only,depth) AS (
  SELECT COALESCE(a.package,?2),?4,0 FROM (SELECT 1) seed
  LEFT JOIN lisp_package_alias a ON a.project=?1 AND a.alias=?2
  UNION
  SELECT COALESCE(a.package,CASE WHEN o.kind='lisp_pkg_use' THEN o.name ELSE o.target END),
         o.kind='lisp_pkg_use',p.depth+1
  FROM search p JOIN lisp_package_options o ON o.project=?1 AND o.package=p.package
  LEFT JOIN lisp_package_alias a ON a.project=?1
    AND a.alias=CASE WHEN o.kind='lisp_pkg_use' THEN o.name ELSE o.target END
  WHERE p.depth<32
    AND (p.external_only=0 OR EXISTS(SELECT 1 FROM lisp_package_options e WHERE e.project=?1 AND e.package=p.package AND e.kind='lisp_pkg_export' AND e.name=?3))
    AND NOT EXISTS(SELECT 1 FROM lisp_package_defs d WHERE d.project=?1 AND d.package=p.package AND d.name=?3)
    AND (o.kind IN ('lisp_pkg_import','lisp_pkg_shadowing_import') AND o.name=?3
      OR o.kind='lisp_pkg_use'
        AND NOT EXISTS(SELECT 1 FROM lisp_package_options e WHERE e.project=?1 AND e.package=p.package AND e.name=?3 AND e.kind IN ('lisp_pkg_shadow','lisp_pkg_intern','lisp_pkg_import','lisp_pkg_shadowing_import')))
)
SELECT d.id FROM search p JOIN lisp_package_defs d ON d.project=?1 AND d.package=p.package AND d.name=?3
WHERE (?5='any' OR d.namespace=?5)
  AND (p.external_only=0 OR EXISTS(SELECT 1 FROM lisp_package_options e WHERE e.project=?1 AND e.package=p.package AND e.kind='lisp_pkg_export' AND e.name=?3));
