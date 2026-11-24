-- Purpose: 순위 검색용 이름·경로 단어와 현재 transaction의 갱신 목록.
-- Pattern: Derived Index.
-- Usage: 분석 인덱스 생성과 ranked_terms의 세대 공개 전 갱신.
-- Related: crates/kodstar-core/src/infrastructure/index/ranked_terms.rs, crates/kodstar-core/src/application/query/ranked.rs.

CREATE TABLE ranked_symbol_terms(term TEXT,id INTEGER REFERENCES symbols_rows(id) ON DELETE CASCADE ON UPDATE CASCADE,
    PRIMARY KEY(term,id)) WITHOUT ROWID;
CREATE INDEX ranked_symbol_terms_owner ON ranked_symbol_terms(id);
CREATE TABLE ranked_file_terms(term TEXT,id INTEGER REFERENCES files_rows(id) ON DELETE CASCADE ON UPDATE CASCADE,
    PRIMARY KEY(term,id)) WITHOUT ROWID;
CREATE INDEX ranked_file_terms_owner ON ranked_file_terms(id);
CREATE TABLE ranked_symbols_pending(id INTEGER PRIMARY KEY REFERENCES symbols_rows(id) ON DELETE CASCADE ON UPDATE CASCADE);
CREATE TABLE ranked_files_pending(id INTEGER PRIMARY KEY REFERENCES files_rows(id) ON DELETE CASCADE ON UPDATE CASCADE);
CREATE TRIGGER ranked_symbol_insert AFTER INSERT ON symbols_rows BEGIN
    INSERT OR IGNORE INTO ranked_symbols_pending VALUES(NEW.id);
END;
CREATE TRIGGER ranked_symbol_update
AFTER UPDATE OF name,syntax_qualified_name,resolved_qualified_name,syntax_kind,resolved_kind ON symbols_rows
WHEN NEW.name<>OLD.name OR NEW.qualified_id<>OLD.qualified_id OR NEW.kind_value<>OLD.kind_value BEGIN
    INSERT OR IGNORE INTO ranked_symbols_pending VALUES(NEW.id);
END;
CREATE TRIGGER ranked_file_insert AFTER INSERT ON files_rows BEGIN
    INSERT OR IGNORE INTO ranked_files_pending VALUES(NEW.id);
END;
CREATE TRIGGER ranked_file_path_update AFTER UPDATE OF path ON files_rows
WHEN NEW.path<>OLD.path BEGIN
    INSERT OR IGNORE INTO ranked_files_pending VALUES(NEW.id);
END;
