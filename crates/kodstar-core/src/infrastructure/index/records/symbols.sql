-- Purpose: 객체의 바이너리 정체성과 공유 이름·서명·원래 구문 값 저장.
-- Pattern: Storage Schema.
-- Usage: records가 논리 symbols projection을 구성하기 전에 실행.
-- Related: crates/kodstar-core/src/infrastructure/index/records/mod.rs, crates/kodstar-core/src/application/analysis/resolve/identities.rs.

CREATE TABLE symbols_rows (
 id INTEGER PRIMARY KEY,file_id INTEGER NOT NULL REFERENCES files_rows(id) ON DELETE CASCADE,
 local_id INTEGER NOT NULL,scope_id INTEGER NOT NULL,
 resolved_key BLOB CHECK(resolved_key IS NULL OR (length(resolved_key)=32 AND resolved_key<>syntax_key)),
 syntax_key BLOB NOT NULL CHECK(length(syntax_key)=32),
 identity BLOB GENERATED ALWAYS AS(COALESCE(resolved_key,syntax_key)) VIRTUAL,
 name INTEGER NOT NULL,name_key INTEGER NOT NULL,
 resolved_qualified_name INTEGER CHECK(resolved_qualified_name IS NULL OR resolved_qualified_name<>syntax_qualified_name),
 syntax_qualified_name INTEGER NOT NULL,
 qualified_id INTEGER GENERATED ALWAYS AS(COALESCE(resolved_qualified_name,syntax_qualified_name)) VIRTUAL,
 resolved_kind INTEGER CHECK(resolved_kind IS NULL OR resolved_kind<>syntax_kind),syntax_kind INTEGER NOT NULL,
 kind_value INTEGER GENERATED ALWAYS AS(COALESCE(resolved_kind,syntax_kind)) VIRTUAL,
 language INTEGER NOT NULL,signature INTEGER NOT NULL,condition INTEGER,
 owner_id INTEGER REFERENCES symbols_rows(id) ON DELETE SET NULL,
 exported INTEGER NOT NULL CHECK(exported IN (0,1)),
 start_byte INTEGER NOT NULL,end_byte INTEGER NOT NULL CHECK(end_byte>=start_byte),
 line INTEGER NOT NULL CHECK(line>0),column_byte INTEGER NOT NULL CHECK(column_byte>0),
 end_line INTEGER NOT NULL CHECK(end_line>0),end_column_byte INTEGER NOT NULL CHECK(end_column_byte>0),
 UNIQUE(file_id,local_id),FOREIGN KEY(file_id,scope_id) REFERENCES scopes(file_id,local_id)
);
CREATE INDEX symbols_name ON symbols_rows(name_key,file_id,scope_id);
CREATE INDEX symbols_qualified ON symbols_rows(qualified_id,file_id);
CREATE INDEX symbols_owner ON symbols_rows(owner_id);
CREATE INDEX symbols_identity ON symbols_rows(identity);
