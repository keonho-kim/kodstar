-- Purpose: 정적 Lisp package alias·전역 정의·옵션의 세대별 디스크 색인.
-- Pattern: SQLite Staging.
-- Usage: 해석 시작 시 고정된 추출 사실에서 재생성.
-- Related: crates/kodstar-core/src/application/analysis/resolve/lisp/mod.rs.

CREATE TEMP TABLE IF NOT EXISTS lisp_package_alias(project TEXT,alias TEXT,package TEXT,PRIMARY KEY(project,alias,package));
DELETE FROM lisp_package_alias;
INSERT OR IGNORE INTO lisp_package_alias
SELECT f.package,b.module_name,b.module_name FROM bindings b JOIN files f ON f.id=b.file_id
WHERE b.kind IN ('lisp_pkg_def','lisp_scope_package');
INSERT OR IGNORE INTO lisp_package_alias
SELECT f.package,b.imported_name,b.module_name FROM bindings b JOIN files f ON f.id=b.file_id
WHERE b.kind='lisp_pkg_nickname';

CREATE TEMP TABLE IF NOT EXISTS lisp_package_defs(project TEXT,package TEXT,name TEXT,namespace TEXT,id INTEGER,PRIMARY KEY(project,package,name,namespace,id));
DELETE FROM lisp_package_defs;
INSERT OR IGNORE INTO lisp_package_defs
SELECT f.package,COALESCE(a.package,p.module_name),b.name_key,b.namespace,s.id
FROM bindings b JOIN files f ON f.id=b.file_id
JOIN symbols s ON s.file_id=b.file_id AND s.local_id=b.target_local
JOIN bindings p ON p.file_id=b.file_id AND p.scope_id=b.scope_id AND p.kind='lisp_scope_package'
LEFT JOIN lisp_package_alias a ON a.project=f.package AND a.alias=p.module_name
WHERE b.kind IN ('lisp_global_function','lisp_global_type','lisp_global_value');

CREATE TEMP TABLE IF NOT EXISTS lisp_package_options(project TEXT,package TEXT,kind TEXT,name TEXT,target TEXT,PRIMARY KEY(project,package,kind,name,target));
DELETE FROM lisp_package_options;
INSERT OR IGNORE INTO lisp_package_options
SELECT f.package,b.module_name,b.kind,b.imported_name,COALESCE(b.type_name,'')
FROM bindings b JOIN files f ON f.id=b.file_id
WHERE b.kind IN ('lisp_pkg_export','lisp_pkg_shadow','lisp_pkg_intern','lisp_pkg_import','lisp_pkg_shadowing_import','lisp_pkg_use');
