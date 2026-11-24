// Purpose: native bridge의 repr(C) 값과 동기 호출 선언.
// Pattern: FFI Contract.
// Usage: callbacks와 안전한 분석 진입점만 접근.
// Related: crates/kodstar-clang/native/bridge.h, crates/kodstar-clang/src/lib.rs.

use std::ffi::{c_char, c_int, c_void};
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Text {
    pub data: *const c_char,
    pub size: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct File {
    pub data: *const u8,
    pub size: usize,
}
#[repr(C)]
pub(super) struct Fact {
    pub kind: u32,
    pub flags: u32,
    pub begin: usize,
    pub end: usize,
    pub name_begin: usize,
    pub name_end: usize,
    pub name: Text,
    pub qualified: Text,
    pub usr: Text,
    pub caller: Text,
    pub target_file: Text,
    pub target_position: usize,
    pub parameter_begin: usize,
    pub parameter_end: usize,
}
#[repr(C)]
pub(super) struct Callbacks {
    pub state: *mut c_void,
    pub read: unsafe extern "C" fn(*mut c_void, Text, c_int, *mut File) -> c_int,
    pub release: unsafe extern "C" fn(*mut c_void, File),
    pub emit: unsafe extern "C" fn(*mut c_void, *const Fact) -> c_int,
    pub check: unsafe extern "C" fn(*mut c_void) -> c_int,
    pub issue: unsafe extern "C" fn(*mut c_void, u32, Text, usize, usize),
}
#[repr(C)]
pub(super) struct Input {
    pub path: Text,
    pub source: Text,
    pub target: Text,
    pub arguments: *const *const c_char,
    pub argument_count: usize,
    pub token_limit: usize,
    pub memory_limit: usize,
    pub template_limit: usize,
    pub path_limit: usize,
    pub cpp: c_int,
}
unsafe extern "C" {
    pub(super) fn ks_clang_analyze(input: *const Input, callbacks: *const Callbacks) -> c_int;
}
