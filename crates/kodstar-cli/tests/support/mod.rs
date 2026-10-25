// Purpose: 임시 프로젝트와 격리한 홈을 사용하는 CLI 테스트 수명.
// Pattern: Test Fixture.
// Usage: commands 통합 테스트가 실제 실행 파일을 호출.
// Related: crates/kodstar-cli/tests/commands.rs.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

pub type TestResult = Result<(), Box<dyn std::error::Error>>;

pub struct Project {
    temporary: Option<tempfile::TempDir>,
    pub root: PathBuf,
    pub home: PathBuf,
}

impl Project {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let temporary = tempfile::Builder::new()
            .prefix("kodstar-cli-test-")
            .tempdir()?;
        let directory = temporary.path().to_path_buf();
        let root = directory.join("project");
        let home = directory.join("home");
        fs::create_dir_all(&root)?;
        fs::create_dir(&home)?;
        Ok(Self {
            temporary: Some(temporary),
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
            .env("ZDOTDIR", &self.home)
            .env("SHELL", "/bin/zsh");
        command
    }

    pub fn run(&self, args: &[&str]) -> std::io::Result<Output> {
        let mut command = self.command();
        command.args(args);
        if args.first() == Some(&"init") {
            command.arg("--yes");
        }
        command.output()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        if let Some(directory) = self.temporary.take()
            && let Err(error) = directory.close()
        {
            if std::thread::panicking() {
                eprintln!("test directory cleanup failed: {error}");
            } else {
                panic!("test directory cleanup failed: {error}");
            }
        }
    }
}
