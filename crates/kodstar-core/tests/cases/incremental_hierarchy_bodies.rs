// Purpose: 선언이 같은 본문 변경에서 타입 계층·owner·dispatch·위치를 재사용하는 계약 검증.
// Pattern: Differential Integration Test.
// Usage: engine이 계층을 가진 여러 언어의 증분·전체 결과를 대조.
// Related: crates/kodstar-core/src/application/analysis/resolve/dispatch.rs, crates/kodstar-core/tests/support/snapshot.rs.

use crate::support::{snapshot::normalized_index, *};
use rusqlite::Connection;

#[test]
fn deleted_repeated_calls_and_bounded_fallback_match_full_analysis() -> TestResult {
    let p = Project::new()?;
    let source = "package api\nfunc A() {}\nfunc B() {}\nfunc caller() { A(); B() }\n";
    p.write("api.go", source)?;
    p.build(false)?;
    for body in ["A();    ", "A(); A()", "A(); B()"] {
        p.write("api.go", &source.replace("A(); B()", body))?;
        let partial = p.build(false)?;
        assert_eq!(
            normalized_index(&partial)?,
            normalized_index(&p.build(true)?)?,
            "{body}"
        );
    }
    let large = source.replace("A(); B()", &"A();".repeat(100));
    p.write("api.go", &large)?;
    p.build(false)?;
    let settings = p.directory.join("small-batches.toml");
    std::fs::write(&settings, "fact_batch_bytes=1024\nqueue_bytes=8192\n")?;
    let mut options = p.options();
    options.config = Some(settings);
    p.write("api.go", &large.replacen("A();", "B();", 1))?;
    let partial = p.engine.init(
        &options,
        kodstar_core::InitOptions::default(),
        &kodstar_core::Cancellation::default(),
        &mut |_| Ok(()),
    )?;
    assert_eq!(
        normalized_index(&partial)?,
        normalized_index(&p.build(true)?)?
    );
    Ok(())
}

#[test]
fn unchanged_calls_in_the_same_file_keep_their_facts_and_relationships() -> TestResult {
    let p = Project::new()?;
    let source = "package api\nfunc A() {}\nfunc B() {}\nfunc C() {}\nfunc caller() { A(); B() }\n";
    p.write("api.go", source)?;
    let original = p.build(false)?;
    let db = Connection::open(std::path::Path::new(&original.cache).join("index.sqlite"))?;
    packed_mutations::mark(&db, "api.go")?;
    postings::register(&db)?;
    let stable = |db: &Connection| {
        db.query_row(
            "SELECT f.id,r.id FROM facts f JOIN relationships r ON r.fact_id=f.id
        WHERE f.relation='calls' AND f.expression='A'",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
        )
    };
    let before = stable(&db)?;
    drop(db);
    p.write("api.go", &source.replace("A(); B()", "A(); C()"))?;
    let partial = p.build(false)?;
    let db = Connection::open(std::path::Path::new(&partial.cache).join("index.sqlite"))?;
    postings::register(&db)?;
    assert_eq!(
        before,
        stable(&db)?,
        "unchanged call in the changed file was replaced"
    );
    drop(db);
    assert_eq!(
        normalized_index(&partial)?,
        normalized_index(&p.build(true)?)?
    );
    Ok(())
}

#[test]
fn stable_declarations_reuse_hierarchies_and_refresh_only_call_effects() -> TestResult {
    for (extension, definitions, caller, stable) in [
        (
            "java",
            "interface Task { void run(); void alt(); } class Worker implements Task { public void run() {} public void alt() {} }",
            "class Use { void go(Task r) { r.run(); } }",
            "class Stable { void help() {} void stay() { help(); } }",
        ),
        (
            "cs",
            "interface Task { void run(); void alt(); } class Worker : Task { public void run() {} public void alt() {} }",
            "class Use { void go(Task r) { r.run(); } }",
            "class Stable { void help() {} void stay() { help(); } }",
        ),
        (
            "rs",
            "pub trait Task { fn run(&self); fn alt(&self); } pub struct Worker; impl Task for Worker { fn run(&self) {} fn alt(&self) {} }",
            "use crate::defs::Task; fn go(r: &dyn Task) { r.run(); }",
            "fn help() {} fn stay() { help(); }",
        ),
        (
            "php",
            "<?php interface Task { public function run(); public function alt(); } class Worker implements Task { public function run() {} public function alt() {} }",
            "<?php function go(Task $r) { $r->run(); }",
            "<?php function help() {} function stay() { help(); }",
        ),
        (
            "py",
            "class Base:\n def run(self): pass\n def alt(self): pass\nclass Worker(Base):\n def run(self): pass\n def alt(self): pass\n",
            "from defs import Base\ndef go(r: Base): r.run()\n",
            "def help(): pass\ndef stay(): help()\n",
        ),
        (
            "cpp",
            "struct Task { virtual void run(); virtual void alt(); }; struct Worker : Task { void run(); void alt(); };",
            "#include \"defs.cpp\"\nvoid go(Task &r) { r.run(); }",
            "void help() {} void stay() { help(); }",
        ),
    ] {
        let p = Project::new()?;
        p.write(&format!("defs.{extension}"), definitions)?;
        let source = format!("caller.{extension}");
        let unrelated = format!("stable.{extension}");
        p.write(&source, caller)?;
        p.write(&unrelated, stable)?;
        let original = p.build(false)?;
        let db = Connection::open(std::path::Path::new(&original.cache).join("index.sqlite"))?;
        packed_mutations::mark(&db, &unrelated)?;
        postings::register(&db)?;
        let stable_id = |db: &Connection| {
            db.query_row("SELECT min(r.id) FROM relationships r
            JOIN facts f ON f.id=r.fact_id JOIN files source ON source.id=f.file_id WHERE source.path=?1",
            [unrelated.as_bytes()], |r| r.get::<_,i64>(0))
        };
        let before = stable_id(&db)?;
        drop(db);
        p.write(&source, &caller.replace("run()", "alt()"))?;
        let partial = p.build(false)?;
        let db = Connection::open(std::path::Path::new(&partial.cache).join("index.sqlite"))?;
        postings::register(&db)?;
        assert_eq!(
            stable_id(&db)?,
            before,
            "{extension}: unrelated graph was replaced"
        );
        drop(db);
        assert_eq!(
            normalized_index(&partial)?,
            normalized_index(&p.build(true)?)?,
            "{extension}"
        );
    }
    Ok(())
}
