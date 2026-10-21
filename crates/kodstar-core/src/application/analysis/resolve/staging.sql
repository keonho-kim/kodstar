-- Purpose: Initialize lookup-local disk candidate tables in the publication transaction.
-- Pattern: Temporary Storage Schema.
-- Usage: resolve starts a generation before evaluating stored facts.
-- Related: crates/kodstar-core/src/application/analysis/resolve/mod.rs.

CREATE TEMP TABLE IF NOT EXISTS resolution_candidates(id INTEGER PRIMARY KEY);
CREATE TEMP TABLE IF NOT EXISTS resolution_modules(depth INTEGER,id INTEGER,PRIMARY KEY(depth,id));
CREATE TEMP TABLE IF NOT EXISTS resolution_saved(lookup_scope INTEGER,id INTEGER,PRIMARY KEY(lookup_scope,id));
CREATE TEMP TABLE IF NOT EXISTS resolution_members(id INTEGER PRIMARY KEY);
CREATE TEMP TABLE IF NOT EXISTS resolution_externals(scope INTEGER,key TEXT,PRIMARY KEY(scope,key));
DELETE FROM resolution_externals;
CREATE TEMP TABLE IF NOT EXISTS resolution_module_owners(depth INTEGER,file_id INTEGER,owner_id INTEGER,PRIMARY KEY(depth,file_id,owner_id));
CREATE TEMP TABLE IF NOT EXISTS resolution_canonical(id INTEGER PRIMARY KEY);
