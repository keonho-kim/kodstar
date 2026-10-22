-- Purpose: 내용 주소 청크와 blob별 순서의 저장 제약.
-- Pattern: Storage Schema.
-- Usage: 신규 이력과 명시적 형식 전환이 동일 DDL을 적용.
-- Related: crates/kodstar-core/src/infrastructure/history/content/chunks.rs.

CREATE TABLE chunk_data(id INTEGER PRIMARY KEY, hash BLOB NOT NULL UNIQUE CHECK(length(hash)=32), size INTEGER NOT NULL CHECK(size BETWEEN 1 AND 65536), codec INTEGER NOT NULL CHECK(codec BETWEEN 0 AND 2), base INTEGER REFERENCES chunk_data(id) CHECK(base<id), lines INTEGER NOT NULL CHECK(lines>=0), first_newline INTEGER NOT NULL, last_newline INTEGER NOT NULL, content BLOB NOT NULL CHECK(length(content) BETWEEN 1 AND 65536), CHECK((codec=2)=(base IS NOT NULL)));
CREATE TABLE chunk_refs(blob INTEGER NOT NULL REFERENCES blobs(id) ON DELETE CASCADE, part INTEGER NOT NULL CHECK(part>=0), start INTEGER NOT NULL CHECK(start>=0), chunk INTEGER NOT NULL REFERENCES chunk_data(id), PRIMARY KEY(blob,part), UNIQUE(blob,start)) WITHOUT ROWID;
CREATE VIEW chunks AS SELECT r.blob,r.part,kodstar_content(d.content,d.codec,d.size,b.content,b.codec,b.size) AS content FROM chunk_refs r JOIN chunk_data d ON d.id=r.chunk LEFT JOIN chunk_data b ON b.id=d.base;
