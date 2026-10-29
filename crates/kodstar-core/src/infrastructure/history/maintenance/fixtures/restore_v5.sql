-- Purpose: 현재 fixture를 형식 1~5의 실제 경로·해시 schema로 되돌림.
-- Pattern: Legacy Test Fixture.
-- Usage: 전환 회귀 검사가 구형 청크 배치를 구성하기 전에 적용.
-- Related: crates/kodstar-core/src/infrastructure/history/maintenance/fixtures/schema_v5.sql.

PRAGMA foreign_keys=OFF;
CREATE TEMP TABLE saved_entries AS SELECT * FROM entries;
DROP TRIGGER entries_insert;
DROP VIEW entries;
DROP TABLE entry_rows;
DROP TABLE paths;
CREATE TABLE entries(checkpoint INTEGER NOT NULL REFERENCES checkpoints(id),path BLOB NOT NULL,blob INTEGER REFERENCES blobs(id),mode INTEGER NOT NULL,kind TEXT NOT NULL,PRIMARY KEY(checkpoint,path)) WITHOUT ROWID;
INSERT INTO entries SELECT * FROM saved_entries;
DROP TABLE saved_entries;
CREATE INDEX entries_path ON entries(path,checkpoint);
CREATE TABLE saved_blobs(id INTEGER PRIMARY KEY,hash TEXT UNIQUE,size INTEGER NOT NULL DEFAULT 0);
INSERT INTO saved_blobs SELECT id,lower(hex(hash)),size FROM blobs;
DROP TABLE blobs;
ALTER TABLE saved_blobs RENAME TO blobs;
PRAGMA foreign_keys=ON;
