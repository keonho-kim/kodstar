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

CREATE TABLE files (
    id INTEGER PRIMARY KEY,
    path BLOB NOT NULL UNIQUE,
    display_path TEXT NOT NULL,
    language TEXT NOT NULL,
    module TEXT NOT NULL,
    declared_module TEXT NOT NULL DEFAULT '',
    path_module TEXT NOT NULL,
    path_package TEXT NOT NULL,
    package TEXT NOT NULL DEFAULT '',
    content_hash BLOB NOT NULL CHECK(length(content_hash)=32),
    facts_hash BLOB NOT NULL DEFAULT X'' CHECK(length(facts_hash) IN (0,32)),
    environment_hash BLOB NOT NULL DEFAULT X'' CHECK(length(environment_hash) IN (0,32)),
    size INTEGER NOT NULL CHECK (size >= 0),
    seen_generation INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('analyzed', 'skipped', 'unsupported', 'auxiliary'))
);
CREATE INDEX files_module ON files(language, package, module);
CREATE INDEX files_generation ON files(seen_generation);

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
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    local_id INTEGER NOT NULL,
    parent INTEGER,
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL CHECK (end_byte >= start_byte),
    PRIMARY KEY (file_id, local_id),
    FOREIGN KEY (file_id, parent) REFERENCES scopes(file_id, local_id)
) WITHOUT ROWID;

CREATE TABLE symbols (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    local_id INTEGER NOT NULL,
    scope_id INTEGER NOT NULL,
    logical_key TEXT NOT NULL,
    syntax_key TEXT NOT NULL,
    name TEXT NOT NULL,
    name_key TEXT NOT NULL,
    qualified_name TEXT NOT NULL,
    syntax_qualified_name TEXT NOT NULL,
    kind TEXT NOT NULL,
    syntax_kind TEXT NOT NULL,
    language TEXT NOT NULL,
    signature TEXT NOT NULL,
    condition TEXT,
    owner_id INTEGER REFERENCES symbols(id) ON DELETE SET NULL,
    exported INTEGER NOT NULL CHECK (exported IN (0, 1)),
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL CHECK (end_byte >= start_byte),
    line INTEGER NOT NULL CHECK (line > 0),
    column_byte INTEGER NOT NULL CHECK (column_byte > 0),
    end_line INTEGER NOT NULL CHECK (end_line > 0),
    end_column_byte INTEGER NOT NULL CHECK (end_column_byte > 0),
    UNIQUE (file_id, local_id),
    FOREIGN KEY (file_id, scope_id) REFERENCES scopes(file_id, local_id)
);
CREATE INDEX symbols_name ON symbols(name_key, file_id, scope_id);
CREATE INDEX symbols_qualified ON symbols(qualified_name, file_id);
CREATE INDEX symbols_owner ON symbols(owner_id);
CREATE INDEX symbols_identity ON symbols(logical_key);

CREATE TABLE symbol_identities (
    symbol_id INTEGER PRIMARY KEY REFERENCES symbols(id) ON DELETE CASCADE,
    canonical_id INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE
);
CREATE INDEX symbol_identities_canonical ON symbol_identities(canonical_id,symbol_id);

CREATE TABLE bindings (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    scope_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    name_key TEXT NOT NULL,
    target_local INTEGER,
    module_name TEXT,
    imported_name TEXT,
    type_name TEXT,
    kind TEXT NOT NULL,
    namespace TEXT NOT NULL CHECK(namespace IN ('any','type','value','macro','function','package')) DEFAULT 'any',
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL CHECK (end_byte >= start_byte),
    FOREIGN KEY (file_id, scope_id) REFERENCES scopes(file_id, local_id),
    FOREIGN KEY (file_id, target_local) REFERENCES symbols(file_id, local_id)
);
CREATE INDEX bindings_lookup ON bindings(file_id, scope_id, name_key, start_byte);
CREATE INDEX bindings_target ON bindings(file_id,target_local,kind);

CREATE TABLE facts (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    source_id INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    scope_id INTEGER NOT NULL,
    expression TEXT NOT NULL,
    relation TEXT NOT NULL,
    language TEXT NOT NULL,
    condition TEXT,
    type_only INTEGER NOT NULL CHECK (type_only IN (0, 1)),
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL CHECK (end_byte >= start_byte),
    line INTEGER NOT NULL CHECK (line > 0),
    column_byte INTEGER NOT NULL CHECK (column_byte > 0),
    end_line INTEGER NOT NULL CHECK (end_line > 0),
    end_column_byte INTEGER NOT NULL CHECK (end_column_byte > 0),
    FOREIGN KEY (file_id, scope_id) REFERENCES scopes(file_id, local_id)
);
CREATE INDEX facts_source ON facts(source_id, relation);
CREATE INDEX facts_file ON facts(file_id, relation);

CREATE TABLE lookup_blocks (
    key_id INTEGER NOT NULL REFERENCES dictionary_ids(id),
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    part INTEGER NOT NULL CHECK(part>=0),
    count INTEGER NOT NULL CHECK(count BETWEEN 1 AND 128),
    payload BLOB NOT NULL CHECK(length(payload) BETWEEN 1 AND 1280),
    PRIMARY KEY (key_id, file_id, part)
) WITHOUT ROWID;
CREATE INDEX lookup_blocks_file ON lookup_blocks(file_id);
CREATE VIEW fact_lookups AS
    SELECT json_extract(j.value,'$[0]') AS fact_id,b.key_id,
           json_extract(j.value,'$[1]') AS found
    FROM lookup_blocks b,json_each(kodstar_postings(b.payload,b.count)) j;
CREATE VIEW lookup_dependencies AS
    SELECT d.fact_id,k.kind,k.lookup_key,d.found
    FROM fact_lookups d JOIN lookup_keys k ON k.id=d.key_id;

CREATE TABLE diagnostics (
    id INTEGER PRIMARY KEY,
    file_id INTEGER REFERENCES files(id) ON DELETE CASCADE,
    code TEXT NOT NULL,
    message TEXT NOT NULL,
    line INTEGER
);
