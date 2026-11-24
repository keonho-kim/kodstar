// Purpose: 제한된 압축 정수 목록을 JSON 변환 없이 SQL 행으로 제공.
// Pattern: Table Valued Adapter.
// Usage: dictionary·dependency·relation codec이 두 정수 열의 decoder를 등록.
// Related: crates/kodstar-core/src/infrastructure/index/dependencies/mod.rs, crates/kodstar-core/src/infrastructure/index/relations/routing/mod.rs.

use rusqlite::{Connection, Result, ffi, vtab::*};
use std::{borrow::Cow, ffi::CStr};

type Decoder = fn(&[u8], i64) -> Result<Vec<[i64; 2]>>;

pub(crate) fn register(c: &Connection, name: &'static CStr, decode: Decoder) -> Result<()> {
    const MODULE: Module<Table> = Module::eponymous_only_module();
    c.create_module(name, &MODULE, Some(decode))
}

#[repr(C)]
struct Table {
    base: ffi::sqlite3_vtab,
    decode: Decoder,
}

// SAFETY: SQLite owns this repr(C) table; the base is first and decoder is a function pointer.
unsafe impl<'vtab> VTab<'vtab> for Table {
    type Aux = Decoder;
    type Cursor = Cursor;

    fn connect(
        db: &mut VTabConnection,
        decode: Option<&Decoder>,
        _: &[u8],
        _: &[u8],
        _: &[u8],
        _: &[&[u8]],
    ) -> Result<(Cow<'static, CStr>, Self)> {
        db.config(VTabConfig::Innocuous)?;
        Ok((
            Cow::Borrowed(c"CREATE TABLE x(first INTEGER,second INTEGER,payload BLOB HIDDEN,parameter INTEGER HIDDEN)"),
            Self {
                base: ffi::sqlite3_vtab::default(),
                decode: *decode.ok_or(rusqlite::Error::InvalidQuery)?,
            },
        ))
    }

    fn best_index(&self, info: &mut IndexInfo) -> Result<bool> {
        let mut supplied = [false; 2];
        for (constraint, mut usage) in info.constraints_and_usages() {
            let column = constraint.column();
            if !(2..=3).contains(&column)
                || constraint.operator() != IndexConstraintOp::SQLITE_INDEX_CONSTRAINT_EQ
            {
                continue;
            }
            if !constraint.is_usable() {
                return Ok(false);
            }
            let index = column as usize - 2;
            if !supplied[index] {
                supplied[index] = true;
                usage.set_argv_index(index as i32 + 1);
                usage.set_omit(true);
            }
        }
        if !supplied.into_iter().all(|v| v) {
            return Ok(false);
        }
        info.set_estimated_rows(128);
        info.set_estimated_cost(16.0);
        Ok(true)
    }

    fn open(&'vtab mut self) -> Result<Cursor> {
        Ok(Cursor {
            base: ffi::sqlite3_vtab_cursor::default(),
            decode: self.decode,
            rows: Vec::new(),
            at: 0,
        })
    }
}

#[repr(C)]
struct Cursor {
    base: ffi::sqlite3_vtab_cursor,
    decode: Decoder,
    rows: Vec<[i64; 2]>,
    at: usize,
}

// SAFETY: The base is first; the cursor owns decoded integers and retains no SQLite pointers.
unsafe impl VTabCursor for Cursor {
    fn filter(&mut self, _: i32, _: Option<&str>, args: &Filters<'_>) -> Result<()> {
        self.rows.clear();
        self.at = 0;
        let bytes: Vec<u8> = args.get(0)?;
        self.rows = (self.decode)(&bytes, args.get(1)?)?;
        if self.rows.len() > 1024 {
            self.rows.clear();
            return Err(rusqlite::Error::InvalidQuery);
        }
        Ok(())
    }
    fn next(&mut self) -> Result<()> {
        self.at += 1;
        Ok(())
    }
    fn eof(&self) -> bool {
        self.at >= self.rows.len()
    }
    fn column(&self, context: &mut Context, column: i32) -> Result<()> {
        let index = usize::try_from(column).map_err(|_| rusqlite::Error::InvalidQuery)?;
        context.set_result(
            self.rows
                .get(self.at)
                .and_then(|row| row.get(index))
                .ok_or(rusqlite::Error::InvalidQuery)?,
        )
    }
    fn rowid(&self) -> Result<i64> {
        Ok(self.at as i64 + 1)
    }
}
