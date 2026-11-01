-- Purpose: 후보 정보의 공유 주소와 블록 단위 참조 수명.
-- Pattern: Storage Schema.
-- Usage: 관계 초기화가 블록 schema 뒤에 적용.
-- Related: crates/kodstar-core/src/infrastructure/index/relations/sets/mod.rs.

CREATE TABLE relation_sets(id INTEGER PRIMARY KEY,digest BLOB NOT NULL UNIQUE CHECK(length(digest)=32),count INTEGER NOT NULL CHECK(count BETWEEN 1 AND 128),payload BLOB NOT NULL,refs INTEGER NOT NULL CHECK(refs>=0));
CREATE TRIGGER relation_sets_insert_guard BEFORE INSERT ON relation_blocks BEGIN
    SELECT CASE WHEN EXISTS(SELECT value FROM json_each(kodstar_relation_sets(NEW.payload,NEW.count)) EXCEPT SELECT id FROM relation_sets) THEN RAISE(ABORT,'missing relationship target set') END;
END;
CREATE TRIGGER relation_sets_update_guard BEFORE UPDATE OF payload,count ON relation_blocks BEGIN
    SELECT CASE WHEN EXISTS(SELECT value FROM json_each(kodstar_relation_sets(NEW.payload,NEW.count)) EXCEPT SELECT id FROM relation_sets) THEN RAISE(ABORT,'missing relationship target set') END;
END;
CREATE TRIGGER relation_sets_insert AFTER INSERT ON relation_blocks BEGIN
    UPDATE relation_sets SET refs=refs+1 WHERE id IN (SELECT value FROM json_each(kodstar_relation_sets(NEW.payload,NEW.count)));
END;
CREATE TRIGGER relation_sets_update AFTER UPDATE OF payload,count ON relation_blocks BEGIN
    UPDATE relation_sets SET refs=refs+1 WHERE id IN (
        SELECT value FROM json_each(kodstar_relation_sets(NEW.payload,NEW.count))
        EXCEPT SELECT value FROM json_each(kodstar_relation_sets(OLD.payload,OLD.count)));
    UPDATE relation_sets SET refs=refs-1 WHERE id IN (
        SELECT value FROM json_each(kodstar_relation_sets(OLD.payload,OLD.count))
        EXCEPT SELECT value FROM json_each(kodstar_relation_sets(NEW.payload,NEW.count)));
    DELETE FROM relation_sets WHERE refs=0 AND id IN (SELECT value FROM json_each(kodstar_relation_sets(OLD.payload,OLD.count)));
END;
CREATE TRIGGER relation_sets_delete AFTER DELETE ON relation_blocks BEGIN
    UPDATE relation_sets SET refs=refs-1 WHERE id IN (SELECT value FROM json_each(kodstar_relation_sets(OLD.payload,OLD.count)));
    DELETE FROM relation_sets WHERE refs=0 AND id IN (SELECT value FROM json_each(kodstar_relation_sets(OLD.payload,OLD.count)));
END;
