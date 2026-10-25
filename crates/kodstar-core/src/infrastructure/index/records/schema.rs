// Purpose: 물리 정수 record의 선택적 문자열 join과 원자적 논리 쓰기 구성.
// Pattern: Storage Projection.
// Usage: 새 index를 만들 때 records가 고정 layout만 전달.
// Related: crates/kodstar-core/src/infrastructure/index/records/mod.rs, crates/kodstar-core/src/infrastructure/index/atoms/mod.rs.

use super::Layout;
use rusqlite::{Connection, Result};

struct Column {
    name: String,
    default: Option<String>,
}

pub(super) fn create(c: &Connection, l: &Layout) -> Result<()> {
    let table = format!("{}_rows", l.name);
    let columns = c
        .prepare(&format!("PRAGMA table_info({table})"))?
        .query_map([], |r| {
            Ok(Column {
                name: r.get(1)?,
                default: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>>>()?;
    let mut select = Vec::new();
    let mut joins = String::new();
    for col in &columns {
        let k = &col.name;
        let expression = if l.atoms.contains(&k.as_str()) {
            if l.indexed.contains(&k.as_str()) {
                joins.push_str(&format!(" LEFT JOIN atoms a_{k} ON a_{k}.id=r.{k}"));
                format!("a_{k}.value")
            } else {
                format!("(SELECT value FROM atoms WHERE id=r.{k})")
            }
        } else if l.digests.contains(&k.as_str()) {
            format!("CASE WHEN r.{k} IS NULL THEN NULL ELSE lower(hex(r.{k})) END")
        } else {
            format!("r.{k}")
        };
        select.push(format!("{expression} AS {k}"));
    }
    for (k, expression) in l.expressions {
        select.push(format!("{expression} AS {k}"));
    }
    for k in l.atoms {
        select.push(format!("r.{k} AS {k}_id"));
    }
    for (k, expression) in l.text_expressions {
        select.push(format!("{expression} AS {k}_id"));
        if l.indexed.contains(k) {
            joins.push_str(&format!(" LEFT JOIN atoms a_{k} ON a_{k}.id={expression}"));
            select.push(format!("a_{k}.value AS {k}"));
        } else {
            select.push(format!(
                "(SELECT value FROM atoms WHERE id={expression}) AS {k}"
            ));
        }
    }
    c.execute_batch(&format!(
        "CREATE VIEW {} AS SELECT {} FROM {table} r {joins};",
        l.name,
        select.join(",")
    ))?;
    let value = |col: &Column, default: bool| {
        let new = format!("NEW.{}", col.name);
        if default {
            col.default
                .as_ref()
                .map_or(new.clone(), |d| format!("COALESCE({new},{d})"))
        } else {
            new
        }
    };
    let atom_values = |default: bool| -> Result<String> {
        Ok(l.atoms
            .iter()
            .map(|k| {
                let col = columns.iter().find(|c| c.name == *k).ok_or_else(|| {
                    rusqlite::Error::ModuleError("unknown atom column in analysis layout".into())
                })?;
                Ok(value(col, default))
            })
            .collect::<Result<Vec<_>>>()?
            .join(","))
    };
    let encoded = |col: &Column, default: bool| {
        let v = value(col, default);
        if l.atoms.contains(&col.name.as_str()) {
            format!("(SELECT id FROM atoms WHERE value={v})")
        } else if l.digests.contains(&col.name.as_str()) {
            format!("kodstar_digest({v})")
        } else {
            v
        }
    };
    let names = columns
        .iter()
        .map(|c| c.name.clone())
        .collect::<Vec<_>>()
        .join(",");
    let insert = columns
        .iter()
        .map(|c| encoded(c, true))
        .collect::<Vec<_>>()
        .join(",");
    let update = columns
        .iter()
        .map(|c| {
            let e = encoded(c, false);
            if l.atoms.contains(&c.name.as_str()) || l.digests.contains(&c.name.as_str()) {
                format!(
                    "{}=CASE WHEN NEW.{} IS OLD.{} THEN {} ELSE {e} END",
                    c.name, c.name, c.name, c.name
                )
            } else {
                format!("{}={e}", c.name)
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    let mut readonly = l
        .expressions
        .iter()
        .chain(l.text_expressions.iter())
        .map(|(k, _)| (*k).to_owned())
        .collect::<Vec<_>>();
    readonly.extend(
        l.atoms
            .iter()
            .chain(l.text_expressions.iter().map(|(k, _)| k))
            .map(|k| format!("{k}_id")),
    );
    let insert_guard = readonly
        .iter()
        .map(|k| format!("NEW.{k} IS NOT NULL"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let insert_guard = if insert_guard.is_empty() {
        String::new()
    } else {
        format!(
            "SELECT CASE WHEN {insert_guard} THEN RAISE(ABORT,'computed analysis fields are read-only') END;"
        )
    };
    c.execute_batch(&format!("
      CREATE TRIGGER {name}_insert INSTEAD OF INSERT ON {name} BEGIN
        {insert_guard}
        INSERT OR IGNORE INTO atoms(value) SELECT value FROM json_each(json_array({insert_atoms})) WHERE value IS NOT NULL;
        INSERT INTO {table}({names}) VALUES({insert}) {conflict}; END;
      CREATE TRIGGER {name}_update INSTEAD OF UPDATE ON {name} BEGIN
        INSERT OR IGNORE INTO atoms(value) SELECT value FROM json_each(json_array({update_atoms})) WHERE value IS NOT NULL;
        UPDATE {table} SET {update} WHERE id=OLD.id; END;
      CREATE TRIGGER {name}_delete INSTEAD OF DELETE ON {name} BEGIN DELETE FROM {table} WHERE id=OLD.id; END;
    ",name=l.name,insert_atoms=atom_values(true)?,update_atoms=atom_values(false)?,conflict=l.conflict))?;
    if !readonly.is_empty() {
        c.execute_batch(&format!("CREATE TRIGGER {name}_readonly INSTEAD OF UPDATE OF {columns} ON {name} BEGIN SELECT RAISE(ABORT,'computed analysis fields are read-only'); END;",name=l.name,columns=readonly.join(",")))?;
    }
    Ok(())
}
