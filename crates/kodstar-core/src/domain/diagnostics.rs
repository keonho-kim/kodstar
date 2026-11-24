// Purpose: 오류 원인·진단·coverage의 구조화된 값.
// Pattern: Simple Module.
// Usage: 공개 API와 내부 실행 경계에서 반환.
// Related: crates/kodstar-core/src/lib.rs.

//! Structured failures. Engine errors never write to a terminal.

use std::error::Error as StdError;
use std::fmt;
use std::path::Path;

mod input;
pub(crate) use input::InputIssue;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    Operation,
    Argument,
    Limit,
    NotFound,
    Cancelled,
}

impl ErrorKind {
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Operation => 1,
            Self::Argument => 2,
            Self::Limit => 3,
            Self::NotFound => 4,
            Self::Cancelled => 130,
        }
    }
}

pub struct Error {
    pub kind: ErrorKind,
    message: String,
    source: Option<Box<dyn StdError + Send + Sync>>,
    private_source: bool,
    pub(crate) input: Option<InputIssue>,
}

impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            source: None,
            private_source: false,
            input: None,
        }
    }

    pub fn argument(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Argument, message)
    }

    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, message)
    }

    pub(crate) fn operation(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Operation, message)
    }

    pub fn caused_by(
        message: impl Into<String>,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind: ErrorKind::Operation,
            message: message.into(),
            source: Some(Box::new(source)),
            private_source: false,
            input: None,
        }
    }

    /// Preserve an input parser's cause for deliberate inspection via `source()` while omitting
    /// its potentially sensitive input excerpt from Display and Debug output.
    pub(crate) fn private_input(
        kind: ErrorKind,
        message: impl Into<String>,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        let mut error = Self::caused_by(message, source);
        error.kind = kind;
        error.private_source = true;
        error
    }

    pub(crate) fn io(action: &str, path: &Path, error: std::io::Error) -> Self {
        Self::caused_by(format!("{action} {}", path.display()), error)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.message)?;
        if let Some(source) = &self.source
            && !self.private_source
        {
            write!(formatter, ": {source}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("Error");
        debug
            .field("kind", &self.kind)
            .field("message", &self.message);
        if self.private_source {
            debug.field("source", &"<input details omitted>");
        } else {
            debug.field("source", &self.source);
        }
        debug.finish()
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source.as_ref().map(|source| source.as_ref() as _)
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
pub struct Coverage {
    pub scanned_files: u64,
    pub analyzed_files: u64,
    pub reused_files: u64,
    pub skipped_files: u64,
    /// Files rejected during extraction; included in skipped_files.
    pub failed_files: u64,
    /// Files with recovered syntax; reliable declarations remain searchable.
    pub partial_files: u64,
    /// Valid preprocessing fragments without a standalone expression context.
    pub fragment_files: u64,
    /// Configuration files with unavailable inherited or referenced settings.
    pub configuration_incomplete_files: u64,
    pub native_incomplete_files: u64,
    pub native_limited_files: u64,
    pub unsupported_files: u64,
    pub source_bytes: u64,
    pub symbols: u64,
    pub relationships: u64,
    pub unresolved: u64,
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<usize>,
}
