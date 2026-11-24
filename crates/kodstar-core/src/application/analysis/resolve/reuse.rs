// Purpose: 위치 구간이 같은 lexical 조회의 결과·부재를 제한 재사용.
// Pattern: Operation Cache.
// Usage: Prepared가 소유하고 사실 해석 중 bindings가 조회.
// Related: crates/kodstar-core/src/application/analysis/resolve/prepared.rs, crates/kodstar-core/src/application/analysis/resolve/bindings.rs.

use super::Binding;
use crate::domain::diagnostics::Result;
use rusqlite::Connection;

pub(super) struct Key<'a> {
    pub scope: u32,
    pub position: usize,
    pub language: &'static str,
    pub namespace: &'static str,
    pub ruby_method: bool,
    pub name: &'a str,
}

struct Entry {
    scope: u32,
    bucket: usize,
    language: &'static str,
    namespace: &'static str,
    ruby_method: bool,
    name: String,
    binding: Option<Binding>,
}

pub(super) struct Cache {
    enabled: bool,
    file: Option<i64>,
    positions: Vec<usize>,
    exact_position: bool,
    entries: Vec<Entry>,
    bytes: usize,
    limit: usize,
    slots: usize,
    pub hits: u64,
    pub misses: u64,
}

impl Cache {
    pub fn new(limit: usize) -> Self {
        Self {
            enabled: false,
            file: None,
            positions: Vec::new(),
            exact_position: true,
            entries: Vec::new(),
            bytes: 0,
            limit,
            slots: (limit / 512).min(128),
            hits: 0,
            misses: 0,
        }
    }

    pub fn enable(&mut self) {
        self.enabled = self.limit >= 4096;
    }

    fn select_file(&mut self, c: &Connection, file: i64) -> Result<()> {
        if self.file == Some(file) {
            return Ok(());
        }
        self.entries.clear();
        self.bytes = 0;
        self.positions.clear();
        self.file = Some(file);
        let count = self.limit / 32;
        let mut select = c.prepare_cached(
            "SELECT DISTINCT start_byte FROM bindings_rows WHERE file_id=?1 ORDER BY start_byte LIMIT ?2",
        )?;
        let mut rows = select.query(rusqlite::params![file, count + 1])?;
        while let Some(row) = rows.next()? {
            if self.positions.len() == count {
                self.positions.clear();
                self.exact_position = true;
                return Ok(());
            }
            self.positions.push(row.get(0)?);
        }
        self.exact_position = false;
        Ok(())
    }

    pub fn binding(
        &mut self,
        c: &Connection,
        file: i64,
        key: Key<'_>,
        load: impl FnOnce() -> Result<Option<Binding>>,
    ) -> Result<Option<Binding>> {
        if !self.enabled {
            return load();
        }
        self.select_file(c, file)?;
        // Query ordering changes only when a binding start crosses the use site.
        // A file-wide partition is conservative for every scope/namespace filter.
        let bucket = if self.exact_position {
            key.position
        } else {
            self.positions.partition_point(|p| *p <= key.position)
        };
        if let Some(at) = self.entries.iter().rposition(|entry| {
            entry.scope == key.scope
                && entry.bucket == bucket
                && entry.language == key.language
                && entry.namespace == key.namespace
                && entry.ruby_method == key.ruby_method
                && entry.name == key.name
        }) {
            self.hits += 1;
            let value = self.entries[at].binding.clone();
            self.entries[at..].rotate_left(1);
            return Ok(value);
        }
        self.misses += 1;
        let value = load()?;
        let owned = key.name.len() + value.as_ref().map_or(0, binding_bytes);
        let overhead = self.positions.capacity() * std::mem::size_of::<usize>()
            + self.slots * std::mem::size_of::<Entry>();
        if owned.saturating_add(overhead) > self.limit {
            return Ok(value);
        }
        while self.entries.len() == self.slots || self.bytes + owned + overhead > self.limit {
            let removed = self.entries.remove(0);
            self.bytes -= removed.name.len() + removed.binding.as_ref().map_or(0, binding_bytes);
        }
        if self.entries.capacity() < self.slots {
            self.entries.reserve_exact(self.slots - self.entries.len());
        }
        self.bytes += owned;
        self.entries.push(Entry {
            scope: key.scope,
            bucket,
            language: key.language,
            namespace: key.namespace,
            ruby_method: key.ruby_method,
            name: key.name.to_owned(),
            binding: value.clone(),
        });
        Ok(value)
    }
}

fn binding_bytes(value: &Binding) -> usize {
    std::mem::size_of::<Binding>()
        + value.kind.len()
        + value.module.as_ref().map_or(0, String::len)
        + value.imported.as_ref().map_or(0, String::len)
        + value.type_name.as_ref().map_or(0, String::len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn optimization_capacity_replaces_one_cold_binding_instead_of_every_entry() -> Result<()> {
        let c = Connection::open_in_memory()?;
        c.execute_batch(
            "CREATE TABLE bindings_rows(file_id INTEGER,start_byte INTEGER);
            INSERT INTO bindings_rows VALUES(1,0);",
        )?;
        let mut cache = Cache::new(4096);
        cache.enable();
        let loads = Cell::new(0);
        let mut get = |name: &str| {
            cache.binding(
                &c,
                1,
                Key {
                    scope: 0,
                    position: 1,
                    language: "rust",
                    namespace: "value",
                    ruby_method: false,
                    name,
                },
                || {
                    loads.set(loads.get() + 1);
                    Ok(None)
                },
            )
        };
        for n in 0..8 {
            get(&format!("n{n}"))?;
        }
        get("n0")?;
        get("n8")?;
        get("n0")?;
        assert_eq!(
            loads.get(),
            9,
            "a hot negative lookup was needlessly discarded"
        );
        Ok(())
    }

    #[test]
    fn use_sites_share_only_stable_intervals_and_keep_absence_distinct() -> Result<()> {
        let c = Connection::open_in_memory()?;
        c.execute_batch(
            "CREATE TABLE bindings_rows(file_id INTEGER,start_byte INTEGER);
            INSERT INTO bindings_rows VALUES(1,0),(1,100),(1,200),(2,0);",
        )?;
        let loads = Cell::new(0);
        let mut cache = Cache::new(65536);
        cache.enable();
        let mut lookup = |file, position, namespace, name| {
            cache.binding(
                &c,
                file,
                Key {
                    scope: 0,
                    position,
                    language: "rust",
                    namespace,
                    ruby_method: false,
                    name,
                },
                || {
                    loads.set(loads.get() + 1);
                    Ok((name != "missing").then(|| Binding {
                        scope: 0,
                        target: Some(if position < 100 { 1 } else { 2 }),
                        module: None,
                        imported: None,
                        type_name: None,
                        kind: "local".into(),
                        position: 0,
                    }))
                },
            )
        };
        assert_eq!(
            lookup(1, 10, "value", "item")?.and_then(|b| b.target),
            Some(1)
        );
        assert_eq!(
            lookup(1, 99, "value", "item")?.and_then(|b| b.target),
            Some(1)
        );
        assert_eq!(loads.get(), 1);
        assert_eq!(
            lookup(1, 100, "value", "item")?.and_then(|b| b.target),
            Some(2)
        );
        assert!(lookup(1, 110, "value", "missing")?.is_none());
        assert!(lookup(1, 150, "value", "missing")?.is_none());
        assert_eq!(loads.get(), 3);
        lookup(1, 150, "type", "missing")?;
        lookup(2, 10, "value", "item")?;
        assert_eq!(loads.get(), 5);
        Ok(())
    }
}
