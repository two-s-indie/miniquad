#![allow(dead_code)]

use miniquad::{gl::*, GlContext};
use std::{
    ffi::c_void,
    sync::{Mutex, MutexGuard},
};

#[derive(Debug, PartialEq)]
pub enum Call {
    Uniform {
        components: usize,
        count: i32,
        pointer: usize,
    },
    TexImage {
        width: i32,
        height: i32,
        format: u32,
        kind: u32,
    },
    ReadPixels {
        width: i32,
        height: i32,
        format: u32,
        kind: u32,
        alignment: i32,
    },
}

static TEST_LOCK: Mutex<()> = Mutex::new(());
static CALLS: Mutex<Vec<Call>> = Mutex::new(Vec::new());
static PACK_ALIGNMENT: Mutex<i32> = Mutex::new(4);
const GL_PACK_ALIGNMENT: u32 = 0x0D05;

pub fn calls() -> Vec<Call> {
    std::mem::take(&mut *CALLS.lock().unwrap())
}

macro_rules! stubs {
    ($($name:ident($($arg:ident: $ty:ty),*);)*) => {
        $(#[allow(non_snake_case, unused_variables)]
        extern "system" fn $name($($arg: $ty),*) {})*

        fn load() {
            load_gl_funcs(|name| {
                let pointer = match name {
                    $(stringify!($name) => $name as *const (),)*
                    "glGetString" => get_string as *const (),
                    "glGetIntegerv" => get_integer as *const (),
                    "glPixelStorei" => pixel_store as *const (),
                    "glGenTextures" | "glGenVertexArrays" | "glGenFramebuffers" => generate as *const (),
                    "glTexImage2D" => tex_image as *const (),
                    "glReadPixels" => read_pixels as *const (),
                    "glCreateShader" => create_shader as *const (),
                    "glCreateProgram" => create_program as *const (),
                    "glGetShaderiv" | "glGetProgramiv" => get_status as *const (),
                    "glGetUniformLocation" => uniform_location as *const (),
                    "glUniform1fv" | "glUniform1iv" => uniform1 as *const (),
                    "glUniform2fv" | "glUniform2iv" => uniform2 as *const (),
                    "glUniform3fv" | "glUniform3iv" => uniform3 as *const (),
                    "glUniform4fv" | "glUniform4iv" => uniform4 as *const (),
                    "glUniformMatrix4fv" => uniform_matrix as *const (),
                    _ => return None,
                };
                Some(unsafe {
                    std::mem::transmute::<*const (), unsafe extern "C" fn()>(pointer)
                })
            });
        }
    }
}

stubs! {
    glBindVertexArray(x: u32); glActiveTexture(x: u32); glBindTexture(t: u32, x: u32);
    glTexParameteri(t: u32, p: u32, x: i32);
    glBindFramebuffer(t: u32, x: u32); glFramebufferTexture2D(t: u32, a: u32, tt: u32, x: u32, l: i32);
    glDeleteFramebuffers(n: i32, x: *const u32); glDeleteTextures(n: i32, x: *const u32);
    glShaderSource(s: u32, n: i32, p: *const *const i8, l: *const i32); glCompileShader(s: u32);
    glDeleteShader(s: u32); glDeleteProgram(p: u32);
    glAttachShader(p: u32, s: u32); glDetachShader(p: u32, s: u32); glLinkProgram(p: u32);
    glUseProgram(p: u32); glEnable(x: u32); glDisable(x: u32); glFrontFace(x: u32);
    glColorMask(r: u8, g: u8, b: u8, a: u8); glDepthFunc(x: u32);
}

extern "system" fn get_string(_: u32) -> *const u8 {
    b"3.3\0".as_ptr()
}
extern "system" fn get_integer(name: u32, value: *mut i32) {
    unsafe {
        *value = if name == GL_PACK_ALIGNMENT {
            *PACK_ALIGNMENT.lock().unwrap()
        } else {
            0
        };
    }
}
extern "system" fn pixel_store(name: u32, value: i32) {
    if name == GL_PACK_ALIGNMENT {
        *PACK_ALIGNMENT.lock().unwrap() = value;
    }
}
extern "system" fn generate(count: i32, values: *mut u32) {
    for i in 0..count {
        unsafe {
            *values.add(i as usize) = i as u32 + 1;
        }
    }
}
extern "system" fn create_shader(kind: u32) -> u32 {
    kind
}
extern "system" fn create_program() -> u32 {
    1
}
extern "system" fn get_status(_: u32, _: u32, value: *mut i32) {
    unsafe {
        *value = 1;
    }
}
extern "system" fn uniform_location(_: u32, name: *const i8) -> i32 {
    if unsafe { std::ffi::CStr::from_ptr(name) }.to_bytes() == b"inactive" {
        -1
    } else {
        0
    }
}
extern "system" fn uniform1(_: i32, count: i32, pointer: *const c_void) {
    record_uniform(1, count, pointer);
}
extern "system" fn uniform2(_: i32, count: i32, pointer: *const c_void) {
    record_uniform(2, count, pointer);
}
extern "system" fn uniform3(_: i32, count: i32, pointer: *const c_void) {
    record_uniform(3, count, pointer);
}
extern "system" fn uniform4(_: i32, count: i32, pointer: *const c_void) {
    record_uniform(4, count, pointer);
}
extern "system" fn uniform_matrix(_: i32, count: i32, _: u8, pointer: *const f32) {
    record_uniform(16, count, pointer as _);
}
fn record_uniform(components: usize, count: i32, pointer: *const c_void) {
    CALLS.lock().unwrap().push(Call::Uniform {
        components,
        count,
        pointer: pointer as usize,
    });
}
extern "system" fn tex_image(
    _: u32,
    _: i32,
    _: i32,
    width: i32,
    height: i32,
    _: i32,
    format: u32,
    kind: u32,
    _: *const c_void,
) {
    CALLS.lock().unwrap().push(Call::TexImage {
        width,
        height,
        format,
        kind,
    });
}
extern "system" fn read_pixels(
    _: i32,
    _: i32,
    width: i32,
    height: i32,
    format: u32,
    kind: u32,
    _: *mut c_void,
) {
    CALLS.lock().unwrap().push(Call::ReadPixels {
        width,
        height,
        format,
        kind,
        alignment: *PACK_ALIGNMENT.lock().unwrap(),
    });
}

pub fn context() -> (MutexGuard<'static, ()>, GlContext) {
    let guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    load();
    *PACK_ALIGNMENT.lock().unwrap() = 4;
    let context = GlContext::new();
    calls();
    (guard, context)
}
