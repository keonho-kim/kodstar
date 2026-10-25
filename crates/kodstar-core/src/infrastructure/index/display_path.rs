// Purpose: 저장된 Unix 경로 바이트의 표시 문자열을 중복 저장 없이 계산.
// Pattern: SQLite Scalar Adapter.
// Usage: index 연결이 schema를 읽기 전에 등록.
// Related: crates/kodstar-core/src/infrastructure/index/mod.rs, crates/kodstar-core/src/infrastructure/index/schema.sql.

use rusqlite::{Connection, Result, functions::FunctionFlags};

pub(super) fn register(connection: &Connection) -> Result<()> {
    connection.create_scalar_function(
        "kodstar_display_path",
        1,
        FunctionFlags::SQLITE_UTF8
            | FunctionFlags::SQLITE_DETERMINISTIC
            | FunctionFlags::SQLITE_INNOCUOUS,
        |context| Ok(String::from_utf8_lossy(context.get_raw(0).as_blob()?).into_owned()),
    )
}
