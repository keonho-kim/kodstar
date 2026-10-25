-- Purpose: 정수 key별 lookup 블록과 계산한 route 그룹의 참조 보호.
-- Pattern: Storage Schema.
-- Usage: index 초기화가 사전·파일 물리 schema와 함께 실행.
-- Related: crates/kodstar-core/src/infrastructure/index/dictionary/schema.sql, crates/kodstar-core/src/infrastructure/index/dependencies/mod.rs.

CREATE TABLE lookup_rows (
 route_group INTEGER GENERATED ALWAYS AS ((key_id-1)/128) VIRTUAL,
 key_id INTEGER NOT NULL CHECK(key_id>0),
 file_id INTEGER NOT NULL REFERENCES files_rows(id) ON DELETE CASCADE,
 part INTEGER NOT NULL CHECK(part>=0),count INTEGER NOT NULL CHECK(count BETWEEN 1 AND 128),
 payload BLOB NOT NULL CHECK(length(payload) BETWEEN 1 AND 1280),
 PRIMARY KEY(key_id,file_id,part)
) WITHOUT ROWID;
CREATE INDEX lookup_blocks_file ON lookup_rows(file_id);
CREATE VIEW lookup_blocks AS SELECT key_id,file_id,part,count,payload,route_group FROM lookup_rows;
CREATE TRIGGER lookup_insert INSTEAD OF INSERT ON lookup_blocks BEGIN
 INSERT INTO lookup_rows(key_id,file_id,part,count,payload) VALUES(NEW.key_id,NEW.file_id,NEW.part,NEW.count,NEW.payload);
END;
CREATE TRIGGER lookup_update INSTEAD OF UPDATE ON lookup_blocks BEGIN
 UPDATE lookup_rows SET key_id=NEW.key_id,file_id=NEW.file_id,part=NEW.part,count=NEW.count,payload=NEW.payload
 WHERE route_group=OLD.route_group AND key_id=OLD.key_id AND file_id=OLD.file_id AND part=OLD.part;
END;
CREATE TRIGGER lookup_delete INSTEAD OF DELETE ON lookup_blocks BEGIN
 DELETE FROM lookup_rows WHERE route_group=OLD.route_group AND key_id=OLD.key_id AND file_id=OLD.file_id AND part=OLD.part;
END;
CREATE TRIGGER lookup_route_insert BEFORE INSERT ON lookup_rows BEGIN
 SELECT CASE WHEN NOT EXISTS(SELECT 1 FROM dictionary_routes p JOIN dictionary_blocks b ON b.id=kodstar_route_at(p.payload,(NEW.key_id-1)%128) WHERE p.id=NEW.route_group)
 THEN RAISE(ABORT,'missing lookup dictionary route') END;
END;
CREATE TRIGGER lookup_route_update BEFORE UPDATE OF key_id ON lookup_rows BEGIN
 SELECT CASE WHEN NOT EXISTS(SELECT 1 FROM dictionary_routes p JOIN dictionary_blocks b ON b.id=kodstar_route_at(p.payload,(NEW.key_id-1)%128) WHERE p.id=NEW.route_group)
 THEN RAISE(ABORT,'missing lookup dictionary route') END;
END;
CREATE TRIGGER dictionary_route_referenced BEFORE DELETE ON dictionary_routes BEGIN
 SELECT CASE WHEN EXISTS(SELECT 1 FROM lookup_rows WHERE key_id BETWEEN OLD.id*128+1 AND min(OLD.id*128+128,9223372036854775807))
 THEN RAISE(ABORT,'dictionary route is still referenced') END;
END;
CREATE TRIGGER dictionary_route_immutable BEFORE UPDATE OF id ON dictionary_routes WHEN NEW.id<>OLD.id BEGIN
 SELECT RAISE(ABORT,'dictionary route groups are immutable');
END;
