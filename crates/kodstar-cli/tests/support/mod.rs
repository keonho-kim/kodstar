// Purpose: 임시 프로젝트와 격리한 홈을 사용하는 CLI 테스트 수명.
// Pattern: Test Fixture.
// Usage: commands 통합 테스트가 실제 실행 파일을 호출.
// Related: crates/kodstar-cli/tests/commands.rs.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub type TestResult = Result<(), Box<dyn std::error::Error>>;
static NEXT: AtomicU64 = AtomicU64::new(0);

pub struct Project {
    directory: PathBuf,
    pub root: PathBuf,
    pub home: PathBuf,
}

impl Project {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "kodstar-cli-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let root = directory.join("project");
        let home = directory.join("home");
        fs::create_dir_all(&root)?;
        fs::create_dir(&home)?;
        Ok(Self {
            directory,
            root,
            home,
        })
    }

    pub fn write(&self, source: &str) -> std::io::Result<()> {
        fs::write(self.root.join("main.rs"), source)
    }

    pub fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_kodstar"));
        command
            .current_dir(&self.root)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home);
        command
    }

    pub fn run(&self, args: &[&str]) -> std::io::Result<Output> {
        self.command().args(args).output()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
