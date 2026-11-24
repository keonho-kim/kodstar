// Purpose: native VFS 입력 버퍼의 조회·소유권 이전·회수 경계.
// Pattern: FFI Input Adapter.
// Usage: callback State가 read·release 함수 포인터를 제공.
// Related: crates/kodstar-clang/src/callbacks.rs, crates/kodstar-clang/src/headers.rs.

use super::{State, text};
use crate::{InputError, Source, Status, ffi, headers};
use std::{
    ffi::{c_int, c_void},
    panic::{AssertUnwindSafe, catch_unwind},
};

pub(super) unsafe extern "C" fn read(
    state: *mut c_void,
    path: ffi::Text,
    content: c_int,
    result: *mut ffi::File,
) -> c_int {
    // State and output pointers are supplied by analyze and the matching native call frame.
    let state = unsafe { &mut *state.cast::<State<'_>>() };
    let outcome = catch_unwind(AssertUnwindSafe(|| -> Result<c_int, InputError> {
        if state.provider.cancelled() {
            return Err(InputError::Cancelled);
        }
        let path = unsafe { text(path) }?;
        let status = if path.starts_with("/__kodstar_clang") {
            headers::read(state.headers, path, false)
        } else {
            state.provider.read(path, false)
        }?;
        let input = if content != 0 {
            if let Source::File { size, .. } = &status {
                state.account(*size, true)?;
            }
            if path.starts_with("/__kodstar_clang") {
                headers::read(state.headers, path, true)
            } else {
                state.provider.read(path, true)
            }?
        } else {
            status
        };
        match input {
            Source::Directory => Ok(1),
            Source::File { size, bytes } => {
                let data = if content != 0 {
                    let mut data = bytes.ok_or(InputError::Failure)?;
                    if data.len() != size {
                        return Err(InputError::Failure);
                    }
                    data.try_reserve_exact(1).map_err(|_| InputError::Failure)?;
                    data.push(0);
                    Box::into_raw(data.into_boxed_slice()).cast::<u8>()
                } else {
                    std::ptr::null_mut()
                };
                unsafe {
                    *result = ffi::File { data, size };
                }
                Ok(0)
            }
        }
    }));
    match outcome {
        Ok(Ok(value)) => value,
        Ok(Err(InputError::Missing)) => -1,
        Ok(Err(error)) => {
            state.failure(error);
            -2
        }
        Err(_) => {
            state.status = Some(Status::Incomplete);
            -2
        }
    }
}

pub(super) unsafe extern "C" fn release(_state: *mut c_void, file: ffi::File) {
    if !file.data.is_null() {
        // read allocates size+1 bytes; the native MemoryBuffer releases it once on drop.
        let slice = std::ptr::slice_from_raw_parts_mut(file.data.cast_mut(), file.size + 1);
        unsafe {
            drop(Box::from_raw(slice));
        }
    }
}
