-- Purpose: 파일 관측 값과 공유된 모듈·package·분류 ID의 물리 저장.
-- Pattern: Storage Schema.
-- Usage: records 초기화가 논리 files view보다 먼저 실행.
-- Related: crates/kodstar-core/src/infrastructure/index/records/mod.rs, crates/kodstar-core/src/infrastructure/index/schema.sql.

CREATE TABLE files_rows (
 id INTEGER PRIMARY KEY,path BLOB NOT NULL UNIQUE,
 language INTEGER NOT NULL,module INTEGER NOT NULL,declared_module INTEGER NOT NULL DEFAULT '',
 path_module INTEGER NOT NULL,path_package INTEGER NOT NULL,package INTEGER NOT NULL DEFAULT '',
 content_hash BLOB NOT NULL CHECK(length(content_hash)=32),
 facts_hash BLOB NOT NULL DEFAULT X'' CHECK(length(facts_hash) IN (0,32)),
 environment_hash BLOB NOT NULL DEFAULT X'' CHECK(length(environment_hash) IN (0,32)),
 size INTEGER NOT NULL CHECK(size>=0),status INTEGER NOT NULL
);
CREATE INDEX files_module ON files_rows(language,package,module);
CREATE INDEX files_status_package ON files_rows(status,package);
CREATE TRIGGER files_rows_status_insert BEFORE INSERT ON files_rows BEGIN
 SELECT CASE WHEN (SELECT value FROM atoms WHERE id=NEW.status) NOT IN ('analyzed','skipped','unsupported','auxiliary') THEN RAISE(ABORT,'invalid file status') END;
END;
CREATE TRIGGER files_rows_status_update BEFORE UPDATE OF status ON files_rows BEGIN
 SELECT CASE WHEN (SELECT value FROM atoms WHERE id=NEW.status) NOT IN ('analyzed','skipped','unsupported','auxiliary') THEN RAISE(ABORT,'invalid file status') END;
END;
