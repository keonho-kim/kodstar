// Purpose: VFS 읽기·취소·근거 수집 callback의 바이트 예산과 unwind 차단.
// Pattern: FFI Adapter.
// Usage: 안전한 analyze 호출이 한 실행 수명 동안 소유.
// Related: crates/kodstar-clang/src/ffi.rs, crates/kodstar-clang/src/headers.rs.

use super::{
    Fact, Facts, HeaderDecoder, InputError, Inputs, Issue, IssueKind, Limits, Status, ffi,
};
use std::{
    ffi::{c_int, c_void},
    panic::{AssertUnwindSafe, catch_unwind},
};

mod input;

pub(super) struct State<'a> {
    provider: &'a mut dyn Inputs,
    headers: &'a HeaderDecoder,
    pub status: Option<Status>,
    pub facts: Facts,
    pub issue: Option<Issue>,
    pub(super) reusable_declarations: bool,
    input_bytes: usize,
    fact_bytes: usize,
    limits: Limits,
    source_size: usize,
}
impl<'a> State<'a> {
    pub fn new(
        provider: &'a mut dyn Inputs,
        limits: Limits,
        source_size: usize,
        target_size: usize,
        headers: &'a HeaderDecoder,
    ) -> Self {
        Self {
            provider,
            headers,
            status: None,
            facts: Facts::default(),
            issue: None,
            reusable_declarations: true,
            input_bytes: source_size,
            fact_bytes: 0,
            limits,
            source_size: target_size,
        }
    }
    pub fn callbacks(&mut self) -> ffi::Callbacks {
        ffi::Callbacks {
            state: (self as *mut Self).cast(),
            read: input::read,
            release: input::release,
            emit,
            check,
            issue,
        }
    }
    fn account(&mut self, extra: usize, input: bool) -> Result<(), InputError> {
        let (used, limit, kind) = if input {
            (
                &mut self.input_bytes,
                self.limits.input_bytes,
                IssueKind::InputBytes,
            )
        } else {
            (
                &mut self.fact_bytes,
                self.limits.fact_bytes,
                IssueKind::FactBytes,
            )
        };
        *used = used.saturating_add(extra);
        if *used > limit {
            self.issue = Some(Issue {
                kind,
                subject: String::new(),
                used: *used,
                limit,
            });
            return Err(InputError::Limit);
        }
        Ok(())
    }
    fn failure(&mut self, error: InputError) {
        self.status = Some(match error {
            InputError::Limit => Status::Limited,
            InputError::Cancelled => Status::Cancelled,
            _ => Status::Incomplete,
        });
    }
}

unsafe fn text<'a>(value: ffi::Text) -> Result<&'a str, InputError> {
    if value.size == 0 {
        return Ok("");
    }
    if value.data.is_null() {
        return Err(InputError::Failure);
    }
    // The native contract borrows these bytes only for the callback duration.
    let bytes = unsafe { std::slice::from_raw_parts(value.data.cast(), value.size) };
    simdutf8::basic::from_utf8(bytes).map_err(|_| InputError::Failure)
}

unsafe extern "C" fn emit(state: *mut c_void, fact: *const ffi::Fact) -> c_int {
    let state = unsafe { &mut *state.cast::<State<'_>>() };
    let outcome = catch_unwind(AssertUnwindSafe(|| -> Result<(), InputError> {
        let raw = unsafe { &*fact };
        if raw.begin > raw.end
            || raw.end > state.source_size
            || raw.name_end > state.source_size
            || raw.parameter_begin > raw.parameter_end
            || raw.parameter_end > state.source_size
        {
            return Err(InputError::Failure);
        }
        state.account(
            std::mem::size_of::<Fact<'_>>()
                + raw.name.size
                + raw.qualified.size
                + raw.usr.size
                + raw.caller.size
                + raw.target_file.size,
            false,
        )?;
        state.facts.push(Fact {
            kind: raw.kind,
            defined: raw.flags & 1 != 0,
            generated: raw.flags & 2 != 0,
            begin: raw.begin,
            end: raw.end,
            name_begin: raw.name_begin,
            name_end: raw.name_end,
            name: unsafe { text(raw.name) }?,
            qualified: unsafe { text(raw.qualified) }?,
            usr: unsafe { text(raw.usr) }?,
            caller: unsafe { text(raw.caller) }?,
            target_file: unsafe { text(raw.target_file) }?,
            target_position: raw.target_position,
            parameters: (raw.parameter_begin < raw.parameter_end)
                .then_some((raw.parameter_begin, raw.parameter_end)),
        });
        Ok(())
    }));
    match outcome {
        Ok(Ok(())) => 1,
        Ok(Err(error)) => {
            state.failure(error);
            0
        }
        Err(_) => {
            state.status = Some(Status::Incomplete);
            0
        }
    }
}

unsafe extern "C" fn issue(
    state: *mut c_void,
    kind: u32,
    subject: ffi::Text,
    used: usize,
    limit: usize,
) {
    let state = unsafe { &mut *state.cast::<State<'_>>() };
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let kind = match kind {
            1 => IssueKind::MissingHeader,
            2 => IssueKind::CompilerError,
            3 => IssueKind::Tokens,
            4 => IssueKind::Templates,
            5 => IssueKind::Paths,
            6 => IssueKind::NativeMemory,
            7 => IssueKind::AstDepth,
            8 => IssueKind::AstWork,
            9 => IssueKind::ClockDependentInput,
            _ => IssueKind::Provider,
        };
        if kind == IssueKind::ClockDependentInput {
            state.reusable_declarations = false;
            state.status = Some(Status::Incomplete);
        }
        if matches!(
            kind,
            IssueKind::Tokens
                | IssueKind::Templates
                | IssueKind::Paths
                | IssueKind::NativeMemory
                | IssueKind::AstDepth
                | IssueKind::AstWork
        ) {
            state.status = Some(Status::Limited);
            if state.issue.as_ref().is_some_and(|i| {
                matches!(i.kind, IssueKind::CompilerError | IssueKind::MissingHeader)
            }) {
                state.issue = None;
            }
        }
        if state.issue.is_some() {
            return;
        }
        let subject = unsafe { text(subject) }.unwrap_or("");
        let subject = subject
            .chars()
            .filter(|c| !c.is_control())
            .take(512)
            .collect();
        state.issue = Some(Issue {
            kind,
            subject,
            used,
            limit,
        });
    }));
    if outcome.is_err() {
        state.status = Some(Status::Incomplete)
    }
}

unsafe extern "C" fn check(state: *mut c_void) -> c_int {
    let state = unsafe { &mut *state.cast::<State<'_>>() };
    match catch_unwind(AssertUnwindSafe(|| state.provider.cancelled())) {
        Ok(true) => {
            state.status = Some(Status::Cancelled);
            0
        }
        Ok(false) => i32::from(state.status.is_none()),
        Err(_) => {
            state.status = Some(Status::Incomplete);
            0
        }
    }
}
