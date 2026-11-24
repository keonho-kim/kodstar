// Purpose: 기본 엔진 조회·갱신·실패·경로 계약 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트 진입점이 사례 모듈로 연결.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn a_blocked_query_sink_does_not_lock_out_another_reader() -> TestResult {
    use std::{sync::mpsc, time::Duration};

    struct Blocked<'a> {
        began: mpsc::Sender<()>,
        resume: &'a mpsc::Receiver<()>,
    }
    impl QuerySink for Blocked<'_> {
        fn begin(&mut self, _metadata: &QueryMetadata) -> Result<()> {
            self.began
                .send(())
                .map_err(|error| Error::new(ErrorKind::Operation, error.to_string()))?;
            self.resume
                .recv_timeout(Duration::from_secs(5))
                .map_err(|error| Error::new(ErrorKind::Operation, error.to_string()))
        }
        fn row(&mut self, row: &QueryRow) -> Result<()> {
            assert!(matches!(row, QueryRow::Symbol(symbol) if symbol.name == "target"));
            Ok(())
        }
        fn finish(&mut self, _summary: &QuerySummary) -> Result<()> {
            Ok(())
        }
    }

    let p = Project::new()?;
    p.write("main.rs", "fn target() {}")?;
    p.write("alternate.rs", "fn replacement() {}")?;
    let alias = p.root.join("alias.rs");
    std::os::unix::fs::symlink("main.rs", &alias)?;
    p.build(false)?;
    let query = Query::Symbols {
        file: "alias.rs".into(),
    };
    let options = QueryOptions {
        refresh: Refresh::None,
        ..QueryOptions::default()
    };
    let (started, begin) = mpsc::channel();
    let (resume, blocked) = mpsc::channel();
    let (done, observed) = mpsc::channel();
    std::thread::scope(|scope| -> TestResult {
        let project = &p;
        let first_query = &query;
        let first = scope.spawn(move || {
            project.engine.query(
                &project.options(),
                first_query,
                &options,
                &Cancellation::default(),
                &mut Blocked {
                    began: started,
                    resume: &blocked,
                },
            )
        });
        begin.recv_timeout(Duration::from_secs(5))?;
        std::fs::remove_file(&alias)?;
        std::os::unix::fs::symlink("alternate.rs", &alias)?;
        let second = scope.spawn(|| {
            let result = p.query(query.clone());
            done.send(result).expect("second result receiver closed");
        });
        let result = observed.recv_timeout(Duration::from_secs(2));
        resume.send(())?;
        first.join().map_err(|_| "first reader panicked")??;
        second.join().map_err(|_| "second reader panicked")?;
        let result = result??;
        assert!(
            matches!(result.rows.as_slice(), [QueryRow::Symbol(symbol)] if symbol.name == "replacement")
        );
        Ok(())
    })
}

#[test]
fn rust_direction_recursion_and_use_ranges_are_preserved() -> TestResult {
    let project = Project::new()?;
    let source = "pub fn target() {}\npub fn caller() { target(); caller(); }\n";
    project.write("src/main.rs", source)?;
    let report = project.build(false)?;
    assert_eq!(report.parsed_files, 1);
    let deps = project.query(Query::Deps {
        selector: selector("src/main.rs", "caller"),
    })?;
    let edges = relationships(&deps);
    assert_eq!(edges.len(), 2);
    for edge in edges {
        assert_eq!(edge.source.name, "caller");
        assert_eq!(edge.relation, "calls");
        assert_eq!(edge.resolution, "resolved");
        let name = &edge.target.as_ref().ok_or("missing target")?.name;
        assert_eq!(
            &source.as_bytes()[edge.site.span.start_byte..edge.site.span.end_byte],
            name.as_bytes()
        );
    }
    let refs = project.query(Query::Refs {
        selector: selector("src/main.rs", "target"),
    })?;
    assert_eq!(relationships(&refs).len(), 1);
    assert_eq!(relationships(&refs)[0].direction, "in");
    Ok(())
}

#[test]
fn parameter_shadowing_does_not_bind_to_a_same_named_global() -> TestResult {
    let project = Project::new()?;
    project.write(
        "api.py",
        "def target():\n    pass\n\ndef caller(target):\n    target()\n",
    )?;
    project.build(false)?;
    let deps = project.query(Query::Deps {
        selector: selector("api.py", "caller"),
    })?;
    assert!(relationships(&deps).is_empty());
    let explained = project.query(Query::Explain {
        selector: selector("api.py", "caller"),
    })?;
    assert!(relationships(&explained).iter().any(|edge| {
        edge.resolution == "unresolved"
            && edge
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("shadows"))
    }));
    Ok(())
}

#[test]
fn failed_file_uses_a_partial_generation_and_older_source_remains_queryable() -> TestResult {
    let project = Project::new()?;
    project.write("src/main.rs", "pub fn stable() {}\n")?;
    let before = project.build(false)?;
    project.write("src/main.rs", "pub fn broken( {\n")?;
    let partial = project.build(false)?;
    assert_eq!(partial.coverage.partial_files, 1);
    assert_eq!(partial.generation, before.generation + 1);
    let cached = project.query(Query::Symbols {
        file: "src/main.rs".into(),
    })?;
    assert!(cached.rows.is_empty());
    assert!(!cached.summary.ok_or("partial summary")?.complete);
    let mut historical = Collected::default();
    project.engine.query(
        &project.options(),
        &Query::Symbols {
            file: "src/main.rs".into(),
        },
        &QueryOptions {
            revision: Some("c1".into()),
            ..QueryOptions::default()
        },
        &Cancellation::default(),
        &mut historical,
    )?;
    assert!(
        historical
            .rows
            .iter()
            .any(|r| matches!(r, QueryRow::Symbol(s) if s.name == "stable"))
    );
    Ok(())
}

#[test]
fn symlink_roots_reuse_the_initialized_project() -> TestResult {
    let project = Project::new()?;
    project.write("api.py", "def sample(): pass\n")?;
    project.build(false)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let link = project.directory.join("linked-project");
        symlink(&project.root, &link)?;
        let options = ProjectOptions::new(link);
        let report = project.engine.init(
            &options,
            InitOptions::default(),
            &Cancellation::default(),
            &mut |_| Ok(()),
        )?;
        assert!(!report.changed);
    }
    Ok(())
}
