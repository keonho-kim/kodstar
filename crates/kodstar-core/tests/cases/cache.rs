// Purpose: cache 수명 정책과 활성·알 수 없는 데이터 보호 검증.
// Pattern: Integration Test.
// Usage: engine 통합 테스트의 하위 모듈로 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;
use std::fs;

#[test]
fn cache_use_refreshes_its_timestamp_without_changing_registration() -> TestResult {
    let project = Project::new()?;
    project.write("main.rs", "fn target() {}")?;
    let report = project.build(false)?;
    let path = std::path::Path::new(&report.cache).join("project.json");
    let registration = fs::read(&path)?;
    let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1);
    fs::OpenOptions::new()
        .write(true)
        .open(&path)?
        .set_modified(old)?;

    project.query(Query::Symbols {
        file: "main.rs".into(),
    })?;

    assert_eq!(fs::read(&path)?, registration);
    assert!(fs::metadata(&path)?.modified()? > old);
    Ok(())
}

#[test]
fn cache_dry_run_and_cleanup_preserve_configuration_and_unknown_data() -> TestResult {
    let project = Project::new()?;
    project.write("main.rs", "fn target() {}")?;
    let report = project.build(false)?;
    let cache = std::path::PathBuf::from(report.cache);
    let global = project.directory.join("cache/config.toml");
    fs::write(&global, "cache_ttl_days = 0\n")?;
    let mut planned = Collected::default();
    project.engine.cache(
        &project.options(),
        CacheRequest::Gc { dry_run: true },
        &Cancellation::default(),
        &mut planned,
    )?;
    assert!(cache.join("index.sqlite").exists());
    assert!(planned.rows.iter().any(|row| matches!(row, QueryRow::Cache(entry) if entry.action == "would_remove" && entry.reason == Some("expired"))));
    fs::write(cache.join("notes.txt"), "user-owned data")?;
    let mut blocked = Collected::default();
    project.engine.cache(
        &project.options(),
        CacheRequest::Clean { all: true },
        &Cancellation::default(),
        &mut blocked,
    )?;
    assert!(cache.join("notes.txt").exists());
    assert!(cache.join("index.sqlite").exists());
    assert!(
        blocked.rows.iter().any(
            |row| matches!(row, QueryRow::Cache(entry) if entry.reason == Some("unknown_data"))
        )
    );
    fs::remove_file(cache.join("notes.txt"))?;
    let mut removed = Collected::default();
    project.engine.cache(
        &project.options(),
        CacheRequest::Clean { all: false },
        &Cancellation::default(),
        &mut removed,
    )?;
    assert!(!cache.exists());
    assert!(global.exists());
    assert!(project.root.join("main.rs").exists());
    Ok(())
}

#[test]
fn cleanup_cannot_remove_a_cache_held_by_a_reader() -> TestResult {
    let project = Project::new()?;
    project.write("main.rs", "fn target() {}")?;
    let report = project.build(false)?;
    struct Reader<'a> {
        project: &'a Project,
        active: bool,
    }
    impl QuerySink for Reader<'_> {
        fn begin(&mut self, _: &QueryMetadata) -> Result<()> {
            let mut sink = Collected::default();
            self.project.engine.cache(
                &self.project.options(),
                CacheRequest::Clean { all: true },
                &Cancellation::default(),
                &mut sink,
            )?;
            self.active = sink.rows.iter().any(|row| matches!(row, QueryRow::Cache(entry) if entry.active && entry.action == "keep"));
            Ok(())
        }
        fn row(&mut self, _: &QueryRow) -> Result<()> {
            Ok(())
        }
        fn finish(&mut self, _: &QuerySummary) -> Result<()> {
            Ok(())
        }
    }
    let mut reader = Reader {
        project: &project,
        active: false,
    };
    project.engine.query(
        &project.options(),
        &Query::Symbols {
            file: "main.rs".into(),
        },
        &QueryOptions {
            refresh: Refresh::None,
            ..QueryOptions::default()
        },
        &Cancellation::default(),
        &mut reader,
    )?;
    assert!(reader.active);
    assert!(
        std::path::Path::new(&report.cache)
            .join("index.sqlite")
            .exists()
    );
    Ok(())
}

#[test]
fn capacity_gc_removes_the_oldest_unused_cache_first() -> TestResult {
    let project = Project::new()?;
    project.write("first.rs", "fn first() {}")?;
    let first = project.build(false)?;
    let other = project.directory.join("other");
    fs::create_dir(&other)?;
    fs::write(other.join("second.rs"), "fn second() {}")?;
    let mut options = project.options();
    options.root = Some(other);
    let second = project.engine.build(
        &options,
        BuildOptions::default(),
        &Cancellation::default(),
        &mut |_| Ok(()),
    )?;
    for (cache, seconds) in [(&first.cache, 1), (&second.cache, 2)] {
        let path = std::path::Path::new(cache).join("project.json");
        let mut record: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
        record["last_used"] = seconds.into();
        fs::write(&path, serde_json::to_vec(&record)?)?;
        fs::OpenOptions::new()
            .write(true)
            .open(path)?
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds))?;
    }
    let mut total = 0;
    for cache in [&first.cache, &second.cache] {
        for entry in fs::read_dir(cache)? {
            let metadata = entry?.metadata()?;
            if metadata.is_file() {
                total += metadata.len();
            }
        }
    }
    fs::write(
        project.directory.join("cache/config.toml"),
        format!(
            "cache_ttl_days = 100000\ncache_budget_bytes = {}\n",
            total - 1
        ),
    )?;
    let mut collected = Collected::default();
    project.engine.cache(
        &project.options(),
        CacheRequest::Gc { dry_run: false },
        &Cancellation::default(),
        &mut collected,
    )?;
    assert!(!std::path::Path::new(&first.cache).exists());
    assert!(std::path::Path::new(&second.cache).exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn cleanup_preserves_symlink_targets_and_reports_unknown_cache_data() -> TestResult {
    let project = Project::new()?;
    project.write("main.rs", "fn target() {}")?;
    let report = project.build(false)?;
    let outside = project.directory.join("private-data");
    fs::write(&outside, "keep this data")?;
    std::os::unix::fs::symlink(
        &outside,
        std::path::Path::new(&report.cache).join("index.sqlite-wal"),
    )?;
    let mut collected = Collected::default();
    project.engine.cache(
        &project.options(),
        CacheRequest::Clean { all: true },
        &Cancellation::default(),
        &mut collected,
    )?;
    assert_eq!(fs::read_to_string(outside)?, "keep this data");
    assert!(
        collected.rows.iter().any(
            |row| matches!(row, QueryRow::Cache(entry) if entry.reason == Some("unknown_data"))
        )
    );
    assert!(
        std::path::Path::new(&report.cache)
            .join("index.sqlite")
            .exists()
    );
    Ok(())
}

#[test]
fn automatic_gc_protects_the_current_project_and_respects_its_interval() -> TestResult {
    let project = Project::new()?;
    project.write("main.rs", "fn target() {}")?;
    let current = project.build(false)?;
    let other = project.directory.join("other");
    fs::create_dir(&other)?;
    fs::write(other.join("other.rs"), "fn other() {}")?;
    let mut options = project.options();
    options.root = Some(other);
    let stale = project.engine.build(
        &options,
        BuildOptions::default(),
        &Cancellation::default(),
        &mut |_| Ok(()),
    )?;
    fs::write(
        project.directory.join("cache/config.toml"),
        "cache_ttl_days = 0\nauto_gc_interval_seconds = 3600\n",
    )?;
    let query = || Query::Symbols {
        file: "main.rs".into(),
    };
    let first = project.query(query())?;
    assert!(std::path::Path::new(&current.cache).exists());
    assert!(!std::path::Path::new(&stale.cache).exists());
    let first = serde_json::to_value(first.metadata.ok_or("missing metadata")?)?;
    assert_eq!(first["maintenance"]["removed"], 1);
    assert_eq!(first["maintenance"]["complete"], true);
    let next = project.query(query())?;
    let next = serde_json::to_value(next.metadata.ok_or("missing next metadata")?)?;
    assert_eq!(first["maintenance"], next["maintenance"]);
    assert!(project.directory.join("cache/gc-state.json").exists());
    Ok(())
}
