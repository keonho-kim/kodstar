-- Purpose: lexical 이름·모듈·타입의 공유 ID와 scope·위치 저장.
-- Pattern: Storage Schema.
-- Usage: records가 논리 bindings view를 만들기 전에 실행.
-- Related: crates/kodstar-core/src/infrastructure/index/records/mod.rs, crates/kodstar-core/src/infrastructure/index/fact_statements.rs.

CREATE TABLE bindings_rows (
 id INTEGER PRIMARY KEY,file_id INTEGER NOT NULL REFERENCES files_rows(id) ON DELETE CASCADE,
 scope_id INTEGER NOT NULL,name INTEGER NOT NULL,name_key INTEGER NOT NULL,target_local INTEGER,
 module_name INTEGER,imported_name INTEGER,type_name INTEGER,kind INTEGER NOT NULL,
 namespace INTEGER NOT NULL DEFAULT 'any',start_byte INTEGER NOT NULL,end_byte INTEGER NOT NULL CHECK(end_byte>=start_byte),
 FOREIGN KEY(file_id,scope_id) REFERENCES scopes(file_id,local_id),
 FOREIGN KEY(file_id,target_local) REFERENCES symbols_rows(file_id,local_id)
);
CREATE INDEX bindings_lookup ON bindings_rows(file_id,scope_id,name_key,start_byte);
CREATE INDEX bindings_target ON bindings_rows(file_id,target_local,kind);
CREATE TRIGGER bindings_rows_namespace_insert BEFORE INSERT ON bindings_rows BEGIN
 SELECT CASE WHEN (SELECT value FROM atoms WHERE id=NEW.namespace) NOT IN ('any','type','value','macro','function','package') THEN RAISE(ABORT,'invalid binding namespace') END;
END;
CREATE TRIGGER bindings_rows_namespace_update BEFORE UPDATE OF namespace ON bindings_rows BEGIN
 SELECT CASE WHEN (SELECT value FROM atoms WHERE id=NEW.namespace) NOT IN ('any','type','value','macro','function','package') THEN RAISE(ABORT,'invalid binding namespace') END;
END;
