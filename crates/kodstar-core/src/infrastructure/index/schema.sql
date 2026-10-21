-- Purpose: 분석 사실·객체·관계·세대의 저장 제약과 인덱스.
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
    content_hash TEXT NOT NULL DEFAULT '',
    facts_hash TEXT NOT NULL DEFAULT '',
    environment_hash TEXT NOT NULL DEFAULT '',
    size INTEGER NOT NULL CHECK (size >= 0),
    modified_ns TEXT NOT NULL,
    changed_ns TEXT NOT NULL,
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
);

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

CREATE TABLE relationships (
    id INTEGER PRIMARY KEY,
    fact_id INTEGER NOT NULL REFERENCES facts(id) ON DELETE CASCADE,
    source_id INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    target_id INTEGER REFERENCES symbols(id) ON DELETE CASCADE,
    external_key TEXT,
    resolution TEXT NOT NULL CHECK (resolution IN ('resolved', 'candidate', 'ambiguous', 'unresolved')),
    rule TEXT NOT NULL,
    reason TEXT,
    CHECK (NOT (target_id IS NOT NULL AND external_key IS NOT NULL)),
    CHECK (resolution <> 'resolved' OR target_id IS NOT NULL OR external_key IS NOT NULL)
);
CREATE INDEX relationships_out ON relationships(source_id, resolution, target_id, fact_id);
CREATE INDEX relationships_in ON relationships(target_id, resolution, source_id, fact_id);
CREATE INDEX relationships_fact ON relationships(fact_id);

CREATE TABLE lookup_dependencies (
    fact_id INTEGER NOT NULL REFERENCES facts(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    lookup_key TEXT NOT NULL,
    found INTEGER NOT NULL CHECK (found IN (0, 1)),
    PRIMARY KEY (fact_id, kind, lookup_key)
);
CREATE INDEX lookup_dependencies_key ON lookup_dependencies(kind, lookup_key, fact_id);

CREATE TABLE diagnostics (
    id INTEGER PRIMARY KEY,
    file_id INTEGER REFERENCES files(id) ON DELETE CASCADE,
    code TEXT NOT NULL,
    message TEXT NOT NULL,
    line INTEGER
);
