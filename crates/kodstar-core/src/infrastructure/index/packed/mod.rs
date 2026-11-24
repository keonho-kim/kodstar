// Purpose: 압축 사전·관계에 대한 SQLite 조건 전달과 한 블록 cursor.
// Pattern: Virtual Table Adapter.
// Usage: index connection이 두 실제 저장 모듈을 등록.
// Related: crates/kodstar-core/src/infrastructure/index/dictionary/mod.rs, crates/kodstar-core/src/infrastructure/index/relations/mod.rs.

pub(crate) mod codec;
mod cursor;
pub(in crate::infrastructure::index) mod expansion;
mod rows;
pub(in crate::infrastructure::index) mod statements;
pub(super) use rows::{Rows, key_matches, matches};

use rusqlite::{
    Connection, Result, ffi,
    types::Value,
    vtab::{self, *},
};
use std::{borrow::Cow, ffi::CStr, rc::Rc};

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Dictionary,
    Relations,
    Dependencies,
}
pub(super) type Filter = (usize, Value);

pub(super) fn register(connection: &Connection) -> Result<()> {
    super::dictionary::routing::register(connection)?;
    super::relations::register(connection)?;
    super::dependencies::pages::register(connection)?;
    connection.execute_batch(super::relations::PRIOR_SCHEMA)?;
    const KEYS: Module<Table> = Module::eponymous_only_module();
    const EDGES: Module<Table> = Module::update_module();
    connection.create_module(c"kodstar_keys", &KEYS, Some(Kind::Dictionary))?;
    connection.create_module(c"kodstar_relations", &EDGES, Some(Kind::Relations))?;
    connection.create_module(c"kodstar_lookups", &EDGES, Some(Kind::Dependencies))
}

#[repr(C)]
struct Table {
    base: ffi::sqlite3_vtab,
    connection: Rc<Connection>,
    kind: Kind,
}

// SAFETY: repr(C), sqlite3_vtab is first; SQLite owns the table until all cursors close.
unsafe impl<'vtab> VTab<'vtab> for Table {
    type Aux = Kind;
    type Cursor = cursor::Cursor;

    fn connect(
        db: &mut VTabConnection,
        kind: Option<&Kind>,
        _: &[u8],
        _: &[u8],
        _: &[u8],
        _: &[&[u8]],
    ) -> Result<(Cow<'static, CStr>, Self)> {
        db.config(VTabConfig::Innocuous)?;
        let kind = *kind.ok_or_else(codec::invalid)?;
        // SAFETY: SQLite invokes this on a live handle. from_handle is non-owning;
        // callbacks stay on that connection's thread, retain no statements, never close it.
        // Each cursor clones the Rc, so xUpdate cannot alias a Rust borrow into the table.
        let connection = Rc::new(unsafe { Connection::from_handle(db.handle())? });
        connection.set_prepared_statement_cache_capacity(4);
        let schema = match kind {
            Kind::Dictionary => c"CREATE TABLE x(id INTEGER,kind TEXT,lookup_key TEXT)",
            Kind::Relations => c"CREATE TABLE x(id INTEGER,fact_id INTEGER,source_id INTEGER,target_id INTEGER,external_key TEXT,resolution TEXT,rule_id INTEGER,reason_id INTEGER)",
            Kind::Dependencies => c"CREATE TABLE x(route_group INTEGER,key_id INTEGER,file_id INTEGER,part INTEGER,count INTEGER,payload BLOB)",
        };
        Ok((
            Cow::Borrowed(schema),
            Self {
                base: ffi::sqlite3_vtab::default(),
                connection,
                kind,
            },
        ))
    }

    fn best_index(&self, info: &mut IndexInfo) -> Result<bool> {
        let binary = info
            .constraints()
            .enumerate()
            .map(|(i, _)| info.collation(i).map(|name| name == "BINARY"))
            .collect::<Result<Vec<_>>>()?;
        let mut columns = Vec::new();
        for (index, (constraint, mut usage)) in info.constraints_and_usages().enumerate() {
            let column = constraint.column();
            let supported = match self.kind {
                Kind::Dictionary => (0..=2).contains(&column) && (column == 0 || binary[index]),
                Kind::Relations => (0..=3).contains(&column) || column == 6,
                Kind::Dependencies => (0..=3).contains(&column),
            };
            if supported
                && constraint.is_usable()
                && constraint.operator() == IndexConstraintOp::SQLITE_INDEX_CONSTRAINT_EQ
                && !columns.contains(&column)
            {
                columns.push(column);
                usage.set_argv_index(columns.len() as i32);
                // SQLite rechecks types/collations: routing must never change SQL equality.
            }
        }
        info.set_idx_str(&format!(
            "{}:{}",
            info.col_used(),
            columns
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ));
        let exact = !matches!(self.kind, Kind::Dependencies) && columns.contains(&0);
        info.set_estimated_rows(if exact {
            1
        } else if columns.is_empty() {
            100_000
        } else {
            32
        });
        info.set_estimated_cost(if exact {
            2.0
        } else if columns.is_empty() {
            100_000.0
        } else {
            16.0
        });
        if exact {
            info.set_idx_flags(IndexFlags::SQLITE_INDEX_SCAN_UNIQUE);
        }
        Ok(true)
    }

    fn open(&'vtab mut self) -> Result<Self::Cursor> {
        Ok(cursor::Cursor::new(self.connection.clone(), self.kind))
    }
}

impl CreateVTab<'_> for Table {
    const KIND: VTabKind = VTabKind::Default;
}

impl UpdateVTab<'_> for Table {
    fn delete(&mut self, id: rusqlite::types::ValueRef<'_>) -> Result<()> {
        if matches!(self.kind, Kind::Dependencies) {
            return super::dependencies::pages::remove(&self.connection, id.as_i64()?);
        }
        super::relations::remove(&self.connection, id.as_i64()?)
    }
    fn insert(&mut self, args: &vtab::Inserts<'_>) -> Result<i64> {
        let values = (2..args.len())
            .map(|i| args.get::<Value>(i))
            .collect::<Result<Vec<_>>>()?;
        if matches!(self.kind, Kind::Dependencies) {
            return super::dependencies::pages::insert(&self.connection, values);
        }
        super::relations::insert(&self.connection, values)
    }
    fn update(&mut self, args: &vtab::Updates<'_>) -> Result<()> {
        let old: i64 = args.get(0)?;
        let mut values = (2..args.len())
            .map(|i| args.get::<Value>(i))
            .collect::<Result<Vec<_>>>()?;
        if matches!(self.kind, Kind::Dependencies) {
            if args.get::<i64>(1)? != old {
                return Err(codec::invalid());
            }
            return super::dependencies::pages::update(&self.connection, old, values);
        }
        let rowid: i64 = args.get(1)?;
        if values[0] == Value::Integer(old) {
            values[0] = Value::Integer(rowid);
        }
        super::relations::remove(&self.connection, old)?;
        super::relations::insert(&self.connection, values)?;
        Ok(())
    }
}
