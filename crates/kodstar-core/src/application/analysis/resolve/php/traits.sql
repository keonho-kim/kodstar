-- Purpose: PHP trait 소유·별칭·제외의 세대별 디스크 cache 수명.
-- Pattern: SQLite Staging.
-- Usage: 해석 세대 시작 시 reset하고 첫 PHP 멤버 조회가 채움.
-- Related: crates/kodstar-core/src/application/analysis/resolve/php/traits.rs.

CREATE TEMP TABLE IF NOT EXISTS php_traits(owner INTEGER,trait INTEGER,PRIMARY KEY(owner,trait));
CREATE TEMP TABLE IF NOT EXISTS php_traits_ready(ready INTEGER);
CREATE TEMP TABLE IF NOT EXISTS php_aliases(owner INTEGER,name TEXT,id INTEGER,visibility TEXT,PRIMARY KEY(owner,name,id));
CREATE TEMP TABLE IF NOT EXISTS php_excluded(owner INTEGER,id INTEGER,PRIMARY KEY(owner,id));
CREATE TEMP TABLE IF NOT EXISTS php_trait_receivers(id INTEGER PRIMARY KEY);
CREATE TEMP TABLE IF NOT EXISTS php_trait_methods(id INTEGER PRIMARY KEY);
DELETE FROM php_traits;
DELETE FROM php_traits_ready;
DELETE FROM php_aliases;
DELETE FROM php_excluded;
DELETE FROM php_trait_receivers;
DELETE FROM php_trait_methods;
