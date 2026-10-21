-- Purpose: 경로 해석의 세대별 SQLite 임시 cache 초기화.
-- Pattern: Temporary Storage Schema.
-- Usage: resolve가 같은 갱신 transaction에서 한 번 실행.
-- Related: crates/kodstar-core/src/application/analysis/resolve/mod.rs.

CREATE TEMP TABLE IF NOT EXISTS resolution_configuration_selection(file_id INTEGER PRIMARY KEY,configuration INTEGER);
CREATE TEMP TABLE IF NOT EXISTS resolution_configuration_seen(root INTEGER,id INTEGER,PRIMARY KEY(root,id));
CREATE TEMP TABLE IF NOT EXISTS resolution_path_patterns(root INTEGER,pattern TEXT,configuration INTEGER,PRIMARY KEY(root,pattern));
CREATE TEMP TABLE IF NOT EXISTS resolution_workspace_seen(root INTEGER PRIMARY KEY);
CREATE TEMP TABLE IF NOT EXISTS resolution_workspace_packages(root INTEGER,name TEXT,manifest INTEGER,PRIMARY KEY(root,manifest));
CREATE TEMP TABLE IF NOT EXISTS resolution_workspace_excluded(root INTEGER,manifest INTEGER,PRIMARY KEY(root,manifest));
CREATE TEMP TABLE IF NOT EXISTS resolution_package_candidates(depth INTEGER,manifest INTEGER,PRIMARY KEY(depth,manifest));
CREATE TEMP TABLE IF NOT EXISTS resolution_package_entries(depth INTEGER,priority TEXT,position INTEGER,target TEXT,native INTEGER);
CREATE TEMP TABLE IF NOT EXISTS go_workspace_selection(file INTEGER PRIMARY KEY,workspace INTEGER);
CREATE TEMP TABLE IF NOT EXISTS go_workspace_members(workspace INTEGER,manifest INTEGER,PRIMARY KEY(workspace,manifest));
CREATE TEMP TABLE IF NOT EXISTS go_workspace_ready(workspace INTEGER PRIMARY KEY);
DELETE FROM resolution_configuration_selection;
DELETE FROM resolution_configuration_seen;
DELETE FROM resolution_path_patterns;
DELETE FROM resolution_workspace_seen;
DELETE FROM resolution_workspace_packages;
DELETE FROM resolution_workspace_excluded;
DELETE FROM resolution_package_candidates;
DELETE FROM resolution_package_entries;
DELETE FROM go_workspace_selection;
DELETE FROM go_workspace_members;
DELETE FROM go_workspace_ready;
