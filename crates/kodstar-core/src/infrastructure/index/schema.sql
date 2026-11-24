-- Purpose: 분석 파일·사실·객체·세대의 저장 제약과 인덱스.
-- Pattern: SQLite Schema.
-- Usage: 빈 index를 초기화할 때 index adapter가 실행.
-- Related: crates/kodstar-core/src/infrastructure/index/mod.rs, crates/kodstar-core/src/infrastructure/index/write.rs.

CREATE TABLE metadata (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    root BLOB NOT NULL,
    generation INTEGER NOT NULL DEFAULT 0 CHECK (generation >= 0),
    config_hash TEXT NOT NULL DEFAULT '',
    indexed_at INTEGER NOT NULL DEFAULT 0,
    last_update TEXT NOT NULL DEFAULT 'not_built',
    last_error TEXT,
    checkpoint TEXT,
    repository TEXT,
    engine_version TEXT NOT NULL,
    extraction_version INTEGER NOT NULL,
    relation_registry_version INTEGER NOT NULL
);

CREATE TABLE coverage (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    generation INTEGER NOT NULL,
    scanned_files INTEGER NOT NULL,
    analyzed_files INTEGER NOT NULL,
    skipped_files INTEGER NOT NULL,
    unsupported_files INTEGER NOT NULL,
    source_bytes INTEGER NOT NULL,
    symbols INTEGER NOT NULL,
    relationships INTEGER NOT NULL,
    unresolved INTEGER NOT NULL
);
INSERT INTO coverage VALUES(1,0,0,0,0,0,0,0,0,0);

CREATE TABLE scopes (
    file_id INTEGER NOT NULL REFERENCES files_rows(id) ON DELETE CASCADE,
    local_id INTEGER NOT NULL,
    parent INTEGER,
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL CHECK (end_byte >= start_byte),
    PRIMARY KEY (file_id, local_id),
    FOREIGN KEY (file_id, parent) REFERENCES scopes(file_id, local_id)
) WITHOUT ROWID;

CREATE TABLE symbol_aliases (
    symbol_id INTEGER PRIMARY KEY REFERENCES symbols_rows(id) ON DELETE CASCADE,
    canonical_id INTEGER NOT NULL REFERENCES symbols_rows(id) ON DELETE CASCADE,
    CHECK(symbol_id<>canonical_id)
);
CREATE INDEX symbol_aliases_canonical ON symbol_aliases(canonical_id,symbol_id);
CREATE VIEW symbol_identities AS
    SELECT symbol_id,canonical_id FROM symbol_aliases
    UNION ALL SELECT s.id,s.id FROM symbols s
    WHERE NOT EXISTS(SELECT 1 FROM symbol_aliases a WHERE a.symbol_id=s.id);

CREATE VIEW fact_lookups AS
    SELECT j.first AS fact_id,b.key_id,j.second AS found
    FROM lookup_blocks b,kodstar_posting_rows(b.payload,b.count) j;
CREATE VIEW lookup_dependencies AS
    SELECT d.fact_id,k.kind,k.lookup_key,d.found
    FROM fact_lookups d JOIN lookup_keys k ON k.id=d.key_id;

CREATE TABLE diagnostics (
    id INTEGER PRIMARY KEY,
    file_id INTEGER REFERENCES files_rows(id) ON DELETE CASCADE,
    code TEXT NOT NULL,
    message TEXT NOT NULL,
    line INTEGER
);

CREATE TABLE native_analysis (
    file_id INTEGER PRIMARY KEY REFERENCES files_rows(id) ON DELETE CASCADE,
    environment BLOB NOT NULL CHECK(length(environment)=32),
    status TEXT NOT NULL CHECK(status IN ('complete','incomplete','limited')),
    detail TEXT
);
CREATE TABLE native_inputs (
    file_id INTEGER NOT NULL REFERENCES native_analysis(file_id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    kind INTEGER NOT NULL CHECK(kind IN (0,1,2)),
    hash BLOB,
    PRIMARY KEY(file_id,path),
    CHECK((kind=2 AND hash IS NOT NULL AND length(hash)=32) OR (kind<>2 AND hash IS NULL))
) WITHOUT ROWID;
CREATE INDEX native_input_consumers ON native_inputs(path,file_id);
