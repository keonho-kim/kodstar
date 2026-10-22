-- Purpose: 안정된 조회 key ID와 접두어 블록·희소 검색 인덱스.
-- Pattern: Storage Schema.
-- Usage: index 초기화가 dependencies보다 먼저 실행.
-- Related: crates/kodstar-core/src/infrastructure/index/dictionary/mod.rs.

CREATE TABLE dictionary_blocks(id INTEGER PRIMARY KEY,kind TEXT NOT NULL,first TEXT,last TEXT,count INTEGER NOT NULL CHECK(count BETWEEN 1 AND 32),payload BLOB NOT NULL,digest BLOB,CHECK((digest IS NULL)=(first IS NOT NULL AND last IS NOT NULL)),CHECK(digest IS NULL OR (count=1 AND length(digest)=32)));
CREATE UNIQUE INDEX dictionary_first ON dictionary_blocks(kind,first);
CREATE INDEX dictionary_digest ON dictionary_blocks(kind,digest) WHERE digest IS NOT NULL;
CREATE TABLE dictionary_ids(id INTEGER PRIMARY KEY,block INTEGER NOT NULL REFERENCES dictionary_blocks(id) ON DELETE CASCADE);
CREATE INDEX dictionary_ids_block ON dictionary_ids(block);
CREATE VIEW lookup_keys AS SELECT id,kind,lookup_key FROM kodstar_keys;
