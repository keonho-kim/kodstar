// Purpose: 네 분석 record의 문자열 공유·논리 열과 물리 schema 구성.
// Pattern: Storage Projection.
// Usage: index 초기화와 문자열 무결성 검사가 같은 record 목록 사용.
// Related: crates/kodstar-core/src/infrastructure/index/records/schema.rs, crates/kodstar-core/src/infrastructure/index/records/references.rs.

mod references;
mod schema;
use rusqlite::{Connection, Result};

pub(crate) struct Layout {
    pub name: &'static str,
    pub sql: &'static str,
    pub atoms: &'static [&'static str],
    pub indexed: &'static [&'static str],
    pub digests: &'static [&'static str],
    pub expressions: &'static [(&'static str, &'static str)],
    pub text_expressions: &'static [(&'static str, &'static str)],
    pub conflict: &'static str,
}

pub(crate) const LAYOUTS: &[Layout] = &[
    Layout {
        name: "files",
        indexed: &["language", "package", "module"],
        sql: include_str!("files.sql"),
        atoms: &[
            "language",
            "module",
            "declared_module",
            "path_module",
            "path_package",
            "package",
            "status",
        ],
        digests: &[],
        expressions: &[("display_path", "kodstar_display_path(r.path)")],
        text_expressions: &[],
        conflict: "ON CONFLICT(path) DO UPDATE SET language=excluded.language,module=excluded.module,path_module=excluded.path_module,path_package=excluded.path_package,package=excluded.package,content_hash=excluded.content_hash,size=excluded.size,status=excluded.status",
    },
    Layout {
        name: "symbols",
        indexed: &["name_key", "qualified_name"],
        sql: include_str!("symbols.sql"),
        atoms: &[
            "name",
            "name_key",
            "syntax_qualified_name",
            "resolved_qualified_name",
            "syntax_kind",
            "resolved_kind",
            "language",
            "signature",
            "condition",
        ],
        digests: &["syntax_key", "resolved_key"],
        expressions: &[
            ("logical_key", "lower(hex(r.identity))"),
            ("key_blob", "r.identity"),
            ("syntax_blob", "r.syntax_key"),
        ],
        text_expressions: &[
            ("qualified_name", "r.qualified_id"),
            ("kind", "r.kind_value"),
        ],
        conflict: "",
    },
    Layout {
        name: "bindings",
        indexed: &["name_key", "kind"],
        sql: include_str!("bindings.sql"),
        atoms: &[
            "name",
            "name_key",
            "module_name",
            "imported_name",
            "type_name",
            "kind",
            "namespace",
        ],
        digests: &[],
        expressions: &[],
        text_expressions: &[],
        conflict: "",
    },
    Layout {
        name: "facts",
        indexed: &["relation"],
        sql: include_str!("facts.sql"),
        atoms: &["expression", "relation", "language", "condition"],
        digests: &[],
        expressions: &[],
        text_expressions: &[],
        conflict: "",
    },
];

pub(crate) fn create(c: &Connection) -> Result<()> {
    for layout in LAYOUTS {
        c.execute_batch(layout.sql)?;
    }
    for layout in LAYOUTS {
        schema::create(c, layout)?;
        references::create(c, layout)?;
    }
    Ok(())
}
