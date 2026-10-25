-- Purpose: 구문 사용의 공유 표현식·관계·언어와 원래 사용 위치 저장.
-- Pattern: Storage Schema.
-- Usage: records가 논리 facts view보다 먼저 실행.
-- Related: crates/kodstar-core/src/infrastructure/index/records/mod.rs, crates/kodstar-core/src/infrastructure/index/fact_statements.rs.

CREATE TABLE facts_rows (
 id INTEGER PRIMARY KEY,file_id INTEGER NOT NULL REFERENCES files_rows(id) ON DELETE CASCADE,
 source_id INTEGER NOT NULL REFERENCES symbols_rows(id) ON DELETE CASCADE,scope_id INTEGER NOT NULL,
 expression INTEGER NOT NULL,relation INTEGER NOT NULL,language INTEGER NOT NULL,condition INTEGER,
 type_only INTEGER NOT NULL CHECK(type_only IN (0,1)),
 start_byte INTEGER NOT NULL,end_byte INTEGER NOT NULL CHECK(end_byte>=start_byte),
 line INTEGER NOT NULL CHECK(line>0),column_byte INTEGER NOT NULL CHECK(column_byte>0),
 end_line INTEGER NOT NULL CHECK(end_line>0),end_column_byte INTEGER NOT NULL CHECK(end_column_byte>0),
 FOREIGN KEY(file_id,scope_id) REFERENCES scopes(file_id,local_id)
);
CREATE INDEX facts_source ON facts_rows(source_id,relation);
CREATE INDEX facts_file ON facts_rows(file_id,relation);
