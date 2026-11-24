// Purpose: parser 타입을 노출하지 않는 native 분석 입력·소유 결과 계약.
// Pattern: Value Objects.
// Usage: core adapter가 입력을 제공하고 완성된 의미 근거를 소비.
// Related: crates/kodstar-clang/src/lib.rs, crates/kodstar-clang/src/callbacks.rs.

pub struct Input<'a> {
    pub path: &'a str,
    /// Collect evidence from this included (virtual path, byte size); None selects the main input.
    pub target: Option<(&'a str, usize)>,
    pub source: &'a [u8],
    pub cpp: bool,
    pub arguments: &'a [String],
    pub limits: Limits,
}
/// Per-translation-unit work limits; AST heap usage is not an OS hard memory limit.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub memory_bytes: usize,
    pub input_bytes: usize,
    pub fact_bytes: usize,
    pub tokens: usize,
    pub templates: usize,
    pub paths: usize,
}
impl Limits {
    pub fn for_workspace(bytes: usize) -> Self {
        Self {
            memory_bytes: bytes.saturating_sub((bytes / 4).min(48 * 1024 * 1024)),
            input_bytes: 128 * 1024 * 1024,
            fact_bytes: (bytes / 8).min(16 * 1024 * 1024),
            tokens: 8 * 1024 * 1024,
            templates: 1024 * 1024,
            paths: 65536,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IssueKind {
    MissingHeader,
    CompilerError,
    InputBytes,
    NativeMemory,
    InputFileBytes,
    Observations,
    FactBytes,
    Tokens,
    Templates,
    Paths,
    Provider,
    CompileCommand,
    AstDepth,
    AstWork,
    ClockDependentInput,
}
#[derive(Clone, Debug)]
pub struct Issue {
    pub kind: IssueKind,
    pub subject: String,
    pub used: usize,
    pub limit: usize,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Complete,
    Incomplete,
    Limited,
    Cancelled,
    InvalidInput,
}
#[derive(Clone, Copy, Debug)]
pub struct Fact<'a> {
    /// 1: function; 2: call; 3: enum; 4: unscoped enum member; 5: scoped enum member.
    pub kind: u32,
    pub defined: bool,
    pub generated: bool,
    pub begin: usize,
    pub end: usize,
    pub name_begin: usize,
    pub name_end: usize,
    pub name: &'a str,
    pub qualified: &'a str,
    pub usr: &'a str,
    pub caller: &'a str,
    pub target_file: &'a str,
    pub target_position: usize,
    /// Original spelling range of a function's formal parameter list, including parentheses.
    pub parameters: Option<(usize, usize)>,
}
pub struct Analysis {
    pub status: Status,
    pub facts: crate::Facts,
    pub issue: Option<Issue>,
}
pub enum Source {
    Directory,
    File { size: usize, bytes: Option<Vec<u8>> },
}
#[derive(Debug)]
pub enum InputError {
    Missing,
    Limit,
    Cancelled,
    Failure,
}
/// Reads only the caller's chosen immutable input. A missing path must not fall back to host I/O.
pub trait Inputs {
    fn read(&mut self, path: &str, content: bool) -> Result<Source, InputError>;
    fn cancelled(&self) -> bool;
}
