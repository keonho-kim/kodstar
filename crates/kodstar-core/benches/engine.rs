// Purpose: 실제 저장된 엔진의 조회·무변경 동기화·분리된 이력 diff 성능 측정.
// Pattern: Benchmark Driver.
// Usage: cargo bench -p kodstar-core --bench engine -- --noplot.
// Related: crates/kodstar-core/src/application/engine.rs, docs/development.md.

use criterion::{Criterion, criterion_group, criterion_main};
use kodstar_core::*;
use std::{fs, hint::black_box, time::Duration};

struct Fixture {
    engine: Engine,
    options: ProjectOptions,
    directory: tempfile::TempDir,
    cancel: Cancellation,
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        fs::create_dir(&root).unwrap();
        let mut source = String::from("package bench\n");
        for n in 0..128 {
            source.push_str(&format!("func F{n}() {{"));
            if n < 127 {
                source.push_str(&format!("F{}()", n + 1));
            }
            source.push_str("}\n");
        }
        fs::write(root.join("main.go"), source).unwrap();
        let lines: Vec<_> = (0..1000)
            .map(|n| format!("original line {n:04}\n"))
            .collect();
        fs::write(root.join("text.txt"), lines.concat()).unwrap();
        let engine = Engine::new(directory.path().join("cache")).unwrap();
        let options = ProjectOptions::new(root.clone());
        let cancel = Cancellation::default();
        engine
            .init(&options, InitOptions::default(), &cancel, &mut |_| Ok(()))
            .unwrap();
        let mut changed = lines;
        changed[10] = "first edit\n".into();
        changed[990] = "last edit\n".into();
        fs::write(root.join("text.txt"), changed.concat()).unwrap();
        engine
            .history(
                &options,
                &HistoryRequest::Save {
                    name: "edited".into(),
                },
                &cancel,
                &mut Count(0),
            )
            .unwrap();
        // Publish this checkpoint before timing unchanged synchronization.
        engine
            .init(&options, InitOptions::default(), &cancel, &mut |_| Ok(()))
            .unwrap();
        Self {
            engine,
            options,
            directory,
            cancel,
        }
    }
    fn close(self) {
        self.directory.close().unwrap();
    }
}
struct Count(u64);
impl QuerySink for Count {
    fn begin(&mut self, _: &QueryMetadata) -> Result<()> {
        self.0 = 0;
        Ok(())
    }
    fn row(&mut self, _: &QueryRow) -> Result<()> {
        self.0 += 1;
        Ok(())
    }
    fn finish(&mut self, summary: &QuerySummary) -> Result<()> {
        assert!(summary.complete && !summary.truncated);
        assert_eq!(self.0, summary.result_count);
        Ok(())
    }
}

fn engine(c: &mut Criterion) {
    let fixture = Fixture::new();
    let query = Query::Related {
        selector: Selector {
            file: "main.go".into(),
            name: "F0".into(),
            at: None,
        },
    };
    let options = QueryOptions {
        refresh: Refresh::None,
        depth: Depth::Transitive,
        max_nodes: Limit::Unlimited,
        limit: Limit::Unlimited,
        ..QueryOptions::default()
    };
    let mut group = c.benchmark_group("engine");
    group
        .sample_size(20)
        .warm_up_time(Duration::from_millis(250))
        .measurement_time(Duration::from_secs(2));
    group.bench_function("related_128", |b| {
        b.iter(|| {
            black_box(
                fixture
                    .engine
                    .query(
                        &fixture.options,
                        &query,
                        &options,
                        &fixture.cancel,
                        &mut Count(0),
                    )
                    .unwrap(),
            );
        })
    });
    let diff = HistoryRequest::Diff {
        from: Some("c1".into()),
        to: Some("c2".into()),
        refresh: Refresh::None,
    };
    group.bench_function("diff_separated_1000", |b| {
        b.iter(|| {
            black_box(
                fixture
                    .engine
                    .history(&fixture.options, &diff, &fixture.cancel, &mut Count(0))
                    .unwrap(),
            );
        })
    });
    group.bench_function("synchronize_unchanged", |b| {
        b.iter(|| {
            black_box(
                fixture
                    .engine
                    .init(
                        &fixture.options,
                        InitOptions::default(),
                        &fixture.cancel,
                        &mut |_| Ok(()),
                    )
                    .unwrap(),
            );
        })
    });
    group.finish();
    fixture.close();
}
criterion_group!(benches, engine);
criterion_main!(benches);
