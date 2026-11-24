// Purpose: 멤버 탐색의 별칭·상속·가까운 선언 선택과 무관한 심볼 수에 독립적인 작업량 검증.
// Pattern: Query Regression Test.
// Usage: 실제 C++ 인덱스·상속 관계에 SQLite progress 예산을 적용.
// Related: crates/kodstar-core/src/application/analysis/resolve/members.sql, crates/kodstar-core/src/infrastructure/index/schema.sql.

use crate::infrastructure::index;
use crate::{Cancellation, Engine, InitOptions, ProjectOptions};

#[test]
fn inherited_members_use_sparse_aliases_and_stop_at_nearer_declarations()
-> Result<(), Box<dyn std::error::Error>> {
    for (body, expected) in [("", "Base::run"), ("void run() {}", "Derived::run")] {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path().join("project");
        std::fs::create_dir(&root)?;
        let root = root.canonicalize()?;
        let mut source = (0..1000)
            .map(|id| format!("struct Unrelated{id} {{}}; void touch{id}() {{ missing(); }}\n"))
            .collect::<String>();
        source.push_str(&format!(
            "struct Base; struct Base {{ void run() {{}} }}; struct Derived : Base {{ {body} }};\n"
        ));
        std::fs::write(root.join("main.cpp"), source)?;
        let report = Engine::new(temporary.path().join("cache"))?.init(
            &ProjectOptions::new(root.clone()),
            InitOptions::default(),
            &Cancellation::default(),
            &mut |_| Ok(()),
        )?;
        let c = index::open(
            &std::path::Path::new(&report.cache).join("index.sqlite"),
            &root,
            false,
            1024 * 1024,
        )?;
        c.execute_batch(include_str!("../staging.sql"))?;
        let receiver: i64 = c.query_row(
            "SELECT id FROM symbols WHERE name='Derived' LIMIT 1",
            [],
            |row| row.get(0),
        )?;
        c.execute("INSERT INTO resolution_candidates VALUES(?1)", [receiver])?;
        let mut calls = 0;
        c.progress_handler(
            100,
            Some(move || {
                calls += 1;
                calls > 30
            }),
        )?;
        c.execute(
            include_str!("../members.sql"),
            rusqlite::params!["run", false],
        )?;
        let found: String = c.query_row(
            "SELECT qualified_name FROM symbols WHERE id IN (SELECT id FROM resolution_members)",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(found, expected);
        assert_eq!(
            c.query_row("SELECT count(*) FROM resolution_members", [], |row| row
                .get::<_, u32>(0))?,
            1
        );
        c.progress_handler(0, None::<fn() -> bool>)?;
        drop(c);
        temporary.close()?;
    }
    Ok(())
}
