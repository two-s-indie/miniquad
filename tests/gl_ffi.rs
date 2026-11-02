#![cfg(not(target_arch = "wasm32"))]

use miniquad::gl::*;
use std::{ffi::c_void, sync::Mutex};

static CALLS: Mutex<Vec<(u32, isize, isize, usize, u32)>> = Mutex::new(Vec::new());

// Use native ABI types independently of Miniquad's aliases. No GL driver or
// allocation is needed to check wide sizes, signed offsets, and stack arguments.
unsafe extern "system" fn buffer_data(target: u32, size: isize, data: *const c_void, usage: u32) {
    CALLS
        .lock()
        .unwrap()
        .push((target, 0, size, data as usize, usage));
}

unsafe extern "system" fn buffer_sub_data(
    target: u32,
    offset: isize,
    size: isize,
    data: *const c_void,
) {
    CALLS
        .lock()
        .unwrap()
        .push((target, offset, size, data as usize, 0));
}

#[test]
fn buffer_dispatch_preserves_pointer_sized_arguments() {
    load_gl_funcs(|name| {
        let pointer = match name {
            "glBufferData" => buffer_data as *const (),
            "glBufferSubData" => buffer_sub_data as *const (),
            _ => return None,
        };
        // load_gl_funcs accepts an erased address and restores the native ABI.
        Some(unsafe { std::mem::transmute::<*const (), unsafe extern "C" fn()>(pointer) })
    });

    let data = [1u8, 2, 3, 4];
    let pointer = data.as_ptr().cast();
    let mut expected = Vec::new();
    for size in [0isize, 257, isize::MAX, -1] {
        unsafe {
            glBufferData(GL_ARRAY_BUFFER, size, pointer, GL_STATIC_DRAW);
            glBufferSubData(GL_ARRAY_BUFFER, -size, size, pointer);
        }
        expected.push((GL_ARRAY_BUFFER, 0, size, pointer as usize, GL_STATIC_DRAW));
        expected.push((GL_ARRAY_BUFFER, -size, size, pointer as usize, 0));
    }
    assert_eq!(*CALLS.lock().unwrap(), expected);
}
