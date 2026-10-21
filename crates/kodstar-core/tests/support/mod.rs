// Purpose: 임시 프로젝트와 소형 결과 수집의 테스트 수명.
// Pattern: Test Fixture.
// Usage: engine과 언어·혼합 입력 회귀 사례가 사용.
// Related: crates/kodstar-core/tests/engine.rs.

pub mod snapshot;

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use kodstar_core::*;

pub type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
static NEXT: AtomicU64 = AtomicU64::new(0);
pub struct Project {
    pub directory: PathBuf,
    pub root: PathBuf,
    pub engine: Engine,
}
impl Project {
    pub fn new() -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "kodstar-test-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory)?;
        let root = directory.join("project");
        fs::create_dir(&root)?;
        let engine = Engine::new(directory.join("cache"))?;
        Ok(Self {
            directory,
            root,
            engine,
        })
    }
    pub fn write(&self, path: &str, source: &str) -> std::io::Result<()> {
        let target = self.root.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target, source)
    }
    pub fn options(&self) -> ProjectOptions {
        ProjectOptions::new(self.root.clone())
    }
    pub fn build(&self, full: bool) -> Result<BuildReport> {
        self.engine.build(
            &self.options(),
            BuildOptions {
                full,
                verify: true,
                path: None,
            },
            &Cancellation::default(),
            &mut |_| Ok(()),
        )
    }
    pub fn query(&self, query: Query) -> Result<Collected> {
        let mut sink = Collected::default();
        self.engine.query(
            &self.options(),
            &query,
            &QueryOptions {
                refresh: Refresh::None,
                limit: Limit::Unlimited,
                ..QueryOptions::default()
            },
            &Cancellation::default(),
            &mut sink,
        )?;
        Ok(sink)
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
#[derive(Default)]
pub struct Collected {
    pub metadata: Option<QueryMetadata>,
    pub rows: Vec<QueryRow>,
    pub summary: Option<QuerySummary>,
}
impl QuerySink for Collected {
    fn begin(&mut self, metadata: &QueryMetadata) -> Result<()> {
        self.metadata = Some(metadata.clone());
        Ok(())
    }
    fn row(&mut self, row: &QueryRow) -> Result<()> {
        self.rows.push(row.clone());
        Ok(())
    }
    fn finish(&mut self, summary: &QuerySummary) -> Result<()> {
        self.summary = Some(summary.clone());
        Ok(())
    }
}
pub fn selector(file: &str, name: &str) -> Selector {
    Selector {
        file: PathBuf::from(file),
        name: name.into(),
        at: None,
    }
}
pub fn relationships(sink: &Collected) -> Vec<&Relationship> {
    sink.rows
        .iter()
        .filter_map(|row| {
            if let QueryRow::Relationship(row) = row {
                Some(row.as_ref())
            } else {
                None
            }
        })
        .collect()
}
