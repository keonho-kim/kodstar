-- Purpose: 문자열 단일 저장과 실제 행 참조 수·불변 ID 제약.
-- Pattern: Storage Schema.
-- Usage: records를 만들기 전에 index 초기화가 실행.
-- Related: crates/kodstar-core/src/infrastructure/index/atoms/mod.rs, crates/kodstar-core/src/infrastructure/index/records/references.rs.

CREATE TABLE atoms(id INTEGER PRIMARY KEY,value TEXT NOT NULL UNIQUE,refs INTEGER NOT NULL DEFAULT 0 CHECK(refs>=0));
CREATE INDEX atoms_unused ON atoms(id) WHERE refs=0;
CREATE TRIGGER atoms_immutable BEFORE UPDATE OF id,value ON atoms
WHEN NEW.id IS NOT OLD.id OR NEW.value IS NOT OLD.value BEGIN
    SELECT RAISE(ABORT,'analysis string atoms are immutable');
END;
CREATE TRIGGER atoms_referenced BEFORE DELETE ON atoms WHEN OLD.refs<>0 BEGIN
    SELECT RAISE(ABORT,'analysis string atom is still referenced');
END;
