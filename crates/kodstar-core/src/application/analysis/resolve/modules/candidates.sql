-- Purpose: 모듈 export 후보의 이름·가시성·소유 범위 선택.
-- Pattern: SQLite Query.
-- Usage: exports.rs가 Rust·C/C++ 이름 ID guard와 여섯 조회 매개변수를 제공.
-- Related: crates/kodstar-core/src/application/analysis/resolve/modules/exports.rs.

INSERT OR IGNORE INTO resolution_candidates
SELECT s.id FROM symbols s JOIN resolution_modules m ON m.id=s.file_id
JOIN symbols o ON o.id=s.owner_id
WHERE /* indexed name guard */ m.depth=?2 AND (s.name=?1 OR s.language IN ('rust','kotlin') AND s.name_key=?4) AND o.kind_id IN (SELECT id FROM atoms WHERE value IN ('file','module'))
AND (s.language NOT IN ('javascript','typescript','erlang','elixir','go') OR s.exported=1)
AND (s.language<>'rust' OR s.kind<>'macro')
AND (s.language<>'erlang' OR EXISTS(SELECT 1 FROM bindings b WHERE b.file_id=s.file_id AND b.target_local=s.local_id AND b.namespace IN ('any',?6)))
AND (s.language<>'zig' OR s.exported=1 OR s.file_id=?5)
AND (s.language<>'swift' OR s.exported=1 OR EXISTS(SELECT 1 FROM files caller JOIN files target ON target.id=s.file_id WHERE caller.id=?5 AND caller.module=target.module AND caller.package=target.package))
AND (?3 IS NULL OR EXISTS(SELECT 1 FROM bindings b WHERE b.file_id=s.file_id AND b.target_local=s.local_id AND b.kind='elixir_function' AND CAST(b.imported_name AS INTEGER)<=?3 AND CAST(b.module_name AS INTEGER)>=?3))
AND ((o.kind='file' AND NOT EXISTS(SELECT 1 FROM resolution_module_owners n WHERE n.depth=m.depth AND n.file_id=s.file_id)) OR o.id IN (SELECT owner_id FROM resolution_module_owners WHERE depth=m.depth AND file_id=s.file_id))
