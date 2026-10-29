-- Purpose: 공유 경로와 변경 전후 blob·정수 속성의 영구 보존.
-- Pattern: Storage Schema.
-- Usage: 최초 이력과 명시적 metadata 전환이 적용.
-- Related: crates/kodstar-core/src/infrastructure/history/schema.sql, crates/kodstar-core/src/infrastructure/history/checkpoints/materialize.rs.

CREATE TABLE paths(id INTEGER PRIMARY KEY,value BLOB NOT NULL UNIQUE CHECK(length(value)>0));
CREATE TABLE entry_rows(
 checkpoint INTEGER NOT NULL REFERENCES checkpoints(id),path INTEGER NOT NULL REFERENCES paths(id),
 blob INTEGER REFERENCES blobs(id),attributes INTEGER NOT NULL CHECK(attributes BETWEEN 0 AND 1023),
 before_blob INTEGER REFERENCES blobs(id),before_attributes INTEGER CHECK(before_attributes BETWEEN 0 AND 1023),
 PRIMARY KEY(checkpoint,path),CHECK((before_blob IS NULL)=(before_attributes IS NULL))
) WITHOUT ROWID;
CREATE INDEX entry_rows_path ON entry_rows(path,checkpoint);
CREATE VIEW entries AS SELECT e.checkpoint,p.value AS path,e.blob,e.attributes&511 AS mode,
 CASE WHEN e.attributes&512 THEN 'symlink' ELSE 'file' END AS kind FROM entry_rows e JOIN paths p ON p.id=e.path;
CREATE TRIGGER entries_insert INSTEAD OF INSERT ON entries BEGIN
 SELECT CASE WHEN NEW.kind NOT IN ('file','symlink') OR NEW.mode NOT BETWEEN 0 AND 511 THEN RAISE(ABORT,'invalid history file attributes') END;
 INSERT OR IGNORE INTO paths(value) VALUES(NEW.path);
 INSERT INTO entry_rows(checkpoint,path,blob,attributes,before_blob,before_attributes)
 SELECT NEW.checkpoint,p.id,NEW.blob,NEW.mode|(CASE WHEN NEW.kind='symlink' THEN 512 ELSE 0 END),w.blob,
   CASE WHEN w.blob IS NULL THEN NULL ELSE w.mode|(CASE WHEN w.kind='symlink' THEN 512 ELSE 0 END) END
 FROM paths p LEFT JOIN working w ON w.path=p.value WHERE p.value=NEW.path;
END;
