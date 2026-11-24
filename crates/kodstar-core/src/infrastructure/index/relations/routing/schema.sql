-- Purpose: 역조회 목록을 압축 페이지로 보관하고 블록 변경과 원자적으로 동기화.
-- Pattern: Storage Schema.
-- Usage: 관계 초기화가 등록된 innocuous codec 함수와 함께 적용.
-- Related: crates/kodstar-core/src/infrastructure/index/relations/routing/mod.rs.

CREATE TABLE relation_route_pages(kind INTEGER NOT NULL CHECK(kind IN (1,3,6)),entity INTEGER NOT NULL CHECK(entity>0),page INTEGER NOT NULL CHECK(page>=0),payload BLOB NOT NULL CHECK(length(payload)>0),PRIMARY KEY(kind,entity,page)) WITHOUT ROWID;
CREATE VIEW relation_routes AS SELECT p.kind,p.entity,j.first AS block
    FROM relation_route_pages p,kodstar_relation_posting_rows(p.payload,p.page) j;

CREATE TRIGGER relation_routes_delete INSTEAD OF DELETE ON relation_routes BEGIN
    DELETE FROM relation_route_pages WHERE kind=OLD.kind AND entity=OLD.entity AND page=(OLD.block-1)/kodstar_relation_width()
        AND payload=kodstar_relation_page(X'',(OLD.block-1)%kodstar_relation_width(),1);
    UPDATE relation_route_pages SET payload=kodstar_relation_page(payload,(OLD.block-1)%kodstar_relation_width(),0)
        WHERE kind=OLD.kind AND entity=OLD.entity AND page=(OLD.block-1)/kodstar_relation_width();
END;

CREATE TRIGGER relation_routes_insert_block AFTER INSERT ON relation_blocks BEGIN
    INSERT INTO relation_route_pages(kind,entity,page,payload)
        SELECT first,second,(NEW.id-1)/kodstar_relation_width(),
            kodstar_relation_page(X'',(NEW.id-1)%kodstar_relation_width(),1)
        FROM kodstar_relation_pair_rows(NEW.routes,0) WHERE 1
        ON CONFLICT(kind,entity,page) DO UPDATE SET payload=kodstar_relation_page(relation_route_pages.payload,(NEW.id-1)%kodstar_relation_width(),1);
END;

CREATE TRIGGER relation_routes_update_block AFTER UPDATE OF routes ON relation_blocks WHEN OLD.routes<>NEW.routes BEGIN
    -- Scalar leading-key filters prevent a vector-IN full scan in bundled SQLite.
    DELETE FROM relation_route_pages WHERE page=(OLD.id-1)/kodstar_relation_width()
        AND kind IN (1,3,6) AND entity IN (SELECT second FROM kodstar_relation_pair_rows(OLD.routes,0))
        AND payload=kodstar_relation_page(X'',(OLD.id-1)%kodstar_relation_width(),1)
        AND (kind,entity) IN (
            SELECT first,second FROM kodstar_relation_pair_rows(OLD.routes,0)
            EXCEPT SELECT first,second FROM kodstar_relation_pair_rows(NEW.routes,0));
    UPDATE relation_route_pages SET payload=kodstar_relation_page(payload,(OLD.id-1)%kodstar_relation_width(),0)
        WHERE page=(OLD.id-1)/kodstar_relation_width()
        AND kind IN (1,3,6) AND entity IN (SELECT second FROM kodstar_relation_pair_rows(OLD.routes,0))
        AND (kind,entity) IN (
            SELECT first,second FROM kodstar_relation_pair_rows(OLD.routes,0)
            EXCEPT SELECT first,second FROM kodstar_relation_pair_rows(NEW.routes,0));
    INSERT INTO relation_route_pages(kind,entity,page,payload)
        SELECT first,second,(NEW.id-1)/kodstar_relation_width(),
            kodstar_relation_page(X'',(NEW.id-1)%kodstar_relation_width(),1)
        FROM kodstar_relation_pair_rows(NEW.routes,0) WHERE (first,second) NOT IN (
            SELECT first,second FROM kodstar_relation_pair_rows(OLD.routes,0))
        ON CONFLICT(kind,entity,page) DO UPDATE SET payload=kodstar_relation_page(relation_route_pages.payload,(NEW.id-1)%kodstar_relation_width(),1);
END;

CREATE TRIGGER relation_routes_delete_block BEFORE DELETE ON relation_blocks WHEN EXISTS(SELECT 1 FROM relation_route_pages) BEGIN
    DELETE FROM relation_route_pages WHERE page=(OLD.id-1)/kodstar_relation_width()
        AND kind IN (1,3,6) AND entity IN (SELECT second FROM kodstar_relation_pair_rows(OLD.routes,0))
        AND payload=kodstar_relation_page(X'',(OLD.id-1)%kodstar_relation_width(),1)
        AND (kind,entity) IN (
            SELECT first,second FROM kodstar_relation_pair_rows(OLD.routes,0));
    UPDATE relation_route_pages SET payload=kodstar_relation_page(payload,(OLD.id-1)%kodstar_relation_width(),0)
        WHERE page=(OLD.id-1)/kodstar_relation_width()
        AND kind IN (1,3,6) AND entity IN (SELECT second FROM kodstar_relation_pair_rows(OLD.routes,0))
        AND (kind,entity) IN (
            SELECT first,second FROM kodstar_relation_pair_rows(OLD.routes,0));
END;
