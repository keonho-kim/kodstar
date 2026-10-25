-- Purpose: 관계 블록과 source·target·fact·안정 ID의 검색 경로.
-- Pattern: Storage Schema.
-- Usage: index 초기화와 relations 저장 adapter가 사용.
-- Related: crates/kodstar-core/src/infrastructure/index/relations/mod.rs.

CREATE TABLE relation_texts(id INTEGER PRIMARY KEY,value TEXT NOT NULL UNIQUE);
CREATE TABLE relation_blocks(id INTEGER PRIMARY KEY,file_id INTEGER NOT NULL REFERENCES files_rows(id) ON DELETE CASCADE,source_id INTEGER NOT NULL REFERENCES symbols_rows(id) ON DELETE CASCADE,count INTEGER NOT NULL CHECK(count BETWEEN 1 AND 128),payload BLOB NOT NULL);
CREATE INDEX relation_blocks_file ON relation_blocks(file_id);
CREATE INDEX relation_blocks_source ON relation_blocks(source_id,file_id);
CREATE TABLE relation_ids(id INTEGER PRIMARY KEY,block INTEGER NOT NULL REFERENCES relation_blocks(id) ON DELETE CASCADE);
CREATE INDEX relation_ids_block ON relation_ids(block);
CREATE TABLE relation_routes(kind INTEGER NOT NULL CHECK(kind IN (1,3,6)),entity INTEGER NOT NULL,block INTEGER NOT NULL REFERENCES relation_blocks(id) ON DELETE CASCADE,PRIMARY KEY(kind,entity,block)) WITHOUT ROWID;
CREATE INDEX relation_routes_block ON relation_routes(block);
CREATE VIRTUAL TABLE relationship_rows USING kodstar_relations;
CREATE VIEW relationships AS SELECT r.id,r.fact_id,r.source_id,r.target_id,r.external_key,r.resolution,
    (SELECT value FROM relation_texts WHERE id=r.rule_id) AS rule,
    (SELECT value FROM relation_texts WHERE id=r.reason_id) AS reason FROM relationship_rows r;
CREATE TRIGGER relationship_insert INSTEAD OF INSERT ON relationships BEGIN
    INSERT OR IGNORE INTO relation_texts(value) VALUES(NEW.rule);
    INSERT OR IGNORE INTO relation_texts(value) SELECT NEW.reason WHERE NEW.reason IS NOT NULL;
    INSERT INTO relationship_rows(id,fact_id,source_id,target_id,external_key,resolution,rule_id,reason_id)
    VALUES(NEW.id,NEW.fact_id,NEW.source_id,NEW.target_id,NEW.external_key,NEW.resolution,
        (SELECT id FROM relation_texts WHERE value=NEW.rule),(SELECT id FROM relation_texts WHERE value=NEW.reason));
END;
CREATE TRIGGER relation_fact_delete BEFORE DELETE ON facts_rows WHEN EXISTS(SELECT 1 FROM relation_routes WHERE kind=1 AND entity=OLD.id) BEGIN DELETE FROM relationship_rows WHERE fact_id=OLD.id; END;
CREATE TRIGGER relation_target_delete BEFORE DELETE ON symbols_rows WHEN EXISTS(SELECT 1 FROM relation_routes WHERE kind=3 AND entity=OLD.id) BEGIN DELETE FROM relationship_rows WHERE target_id=OLD.id; END;
