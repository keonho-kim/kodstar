-- Purpose: 영구 소스 이력과 중단 복구의 저장 형식.
-- Pattern: Storage Schema.
-- Usage: history 저장소를 최초 생성할 때 적용.
-- Related: crates/kodstar-core/src/infrastructure/history/mod.rs.

CREATE TABLE repository(singleton INTEGER PRIMARY KEY CHECK(singleton=1), identity TEXT NOT NULL, current_branch TEXT NOT NULL);
CREATE TABLE checkpoints(id INTEGER PRIMARY KEY, parent INTEGER REFERENCES checkpoints(id), created_at INTEGER NOT NULL, changed_files INTEGER NOT NULL, tracked_files INTEGER NOT NULL);
CREATE TABLE branches(name TEXT PRIMARY KEY, checkpoint INTEGER REFERENCES checkpoints(id));
CREATE TABLE names(name TEXT PRIMARY KEY, checkpoint INTEGER NOT NULL UNIQUE REFERENCES checkpoints(id));
CREATE TABLE blobs(id INTEGER PRIMARY KEY, hash TEXT UNIQUE, size INTEGER NOT NULL DEFAULT 0);
CREATE TABLE entries(checkpoint INTEGER NOT NULL REFERENCES checkpoints(id), path BLOB NOT NULL, blob INTEGER REFERENCES blobs(id), mode INTEGER NOT NULL, kind TEXT NOT NULL, PRIMARY KEY(checkpoint,path)) WITHOUT ROWID;
CREATE TABLE working(path BLOB PRIMARY KEY, blob INTEGER NOT NULL REFERENCES blobs(id), mode INTEGER NOT NULL, kind TEXT NOT NULL, size INTEGER NOT NULL, modified TEXT NOT NULL, changed TEXT NOT NULL) WITHOUT ROWID;
CREATE TABLE checkout(singleton INTEGER PRIMARY KEY CHECK(singleton=1), target INTEGER NOT NULL REFERENCES checkpoints(id), branch TEXT, path BLOB);
CREATE TABLE checkout_files(path BLOB PRIMARY KEY, before_blob INTEGER, before_mode INTEGER, before_kind TEXT, after_blob INTEGER, after_mode INTEGER, after_kind TEXT, done INTEGER NOT NULL DEFAULT 0) WITHOUT ROWID;
CREATE INDEX entries_path ON entries(path,checkpoint);
INSERT INTO repository VALUES(1,'','main');
INSERT INTO branches VALUES('main',NULL);
