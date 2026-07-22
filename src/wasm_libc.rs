// Minimal libc for the C half when targeting wasm32-unknown-unknown, which has no libc.
//
// The decompiled core barely touches libc: malloc/free (5 uses each), memcpy/memset (3 each,
// already supplied by compiler_builtins on wasm32), abs, and sqrtf (a native wasm instruction).
// rand/printf appear only inside `#if ENABLE_TEST_SUITE`, so they are absent from normal builds.

use std::alloc::{alloc, dealloc, Layout};

/// Bytes reserved before each allocation to record its size, so `free` can rebuild the Layout.
/// 16 keeps the returned pointer 16-byte aligned, which is stricter than anything the C needs.
const HEADER: usize = 16;
const ALIGN: usize = 16;

#[no_mangle]
pub unsafe extern "C" fn malloc(size: usize) -> *mut u8 {
    if size == 0 {
        return std::ptr::null_mut();
    }
    let total = match size.checked_add(HEADER) {
        Some(t) => t,
        None => return std::ptr::null_mut(),
    };
    let layout = match Layout::from_size_align(total, ALIGN) {
        Ok(l) => l,
        Err(_) => return std::ptr::null_mut(),
    };
    let base = alloc(layout);
    if base.is_null() {
        return std::ptr::null_mut();
    }
    (base as *mut usize).write(total);
    base.add(HEADER)
}

#[no_mangle]
pub unsafe extern "C" fn free(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    let base = ptr.sub(HEADER);
    let total = (base as *mut usize).read();
    if let Ok(layout) = Layout::from_size_align(total, ALIGN) {
        dealloc(base, layout);
    }
}

#[no_mangle]
pub unsafe extern "C" fn calloc(count: usize, size: usize) -> *mut u8 {
    let total = match count.checked_mul(size) {
        Some(t) => t,
        None => return std::ptr::null_mut(),
    };
    let p = malloc(total);
    if !p.is_null() {
        std::ptr::write_bytes(p, 0, total);
    }
    p
}

#[no_mangle]
pub extern "C" fn abs(v: i32) -> i32 {
    v.wrapping_abs()
}
