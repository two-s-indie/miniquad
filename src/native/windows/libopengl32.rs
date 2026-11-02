#![allow(non_camel_case_types, dead_code, non_snake_case)]

use winapi::shared::{minwindef::*, ntdef::*, windef::*};

// WGL uses WINAPI and a 32-bit BOOL, not Rust's one-byte bool.
pub type wglCreateContext = unsafe extern "system" fn(_: HDC) -> HGLRC;
pub type wglDeleteContext = unsafe extern "system" fn(_: HGLRC) -> BOOL;
pub type wglGetProcAddress = unsafe extern "system" fn(_: LPCSTR) -> PROC;
pub type wglGetCurrentDC = unsafe extern "system" fn() -> HDC;
pub type wglMakeCurrent = unsafe extern "system" fn(_: HDC, _: HGLRC) -> BOOL;

pub struct LibOpengl32 {
    pub module: crate::native::module::Module,
    pub wglCreateContext: wglCreateContext,
    pub wglDeleteContext: wglDeleteContext,
    pub wglGetProcAddress: wglGetProcAddress,
    pub wglGetCurrentDC: wglGetCurrentDC,
    pub wglMakeCurrent: wglMakeCurrent,
}

#[cfg(test)]
mod tests {
    use super::*;
    use winapi::um::wingdi;

    // Check our dynamically loaded signatures against the existing SDK bindings.
    // Unused constants do not add static imports to the production loader.
    const _: wglCreateContext = wingdi::wglCreateContext;
    const _: wglDeleteContext = wingdi::wglDeleteContext;
    const _: wglGetProcAddress = wingdi::wglGetProcAddress;
    const _: wglGetCurrentDC = wingdi::wglGetCurrentDC;
    const _: wglMakeCurrent = wingdi::wglMakeCurrent;

    #[test]
    fn wgl_bool_returns_preserve_all_32_bits() {
        unsafe extern "system" fn make_current(_: HDC, _: HGLRC) -> BOOL {
            0x100
        }
        unsafe extern "system" fn delete_context(_: HGLRC) -> BOOL {
            -1
        }
        let make_current: wglMakeCurrent = make_current;
        let delete_context: wglDeleteContext = delete_context;
        unsafe {
            assert_eq!(
                make_current(std::ptr::null_mut(), std::ptr::null_mut()),
                0x100
            );
            assert_eq!(delete_context(std::ptr::null_mut()), -1);
        }
    }
}

impl LibOpengl32 {
    pub fn try_load() -> Option<LibOpengl32> {
        crate::native::module::Module::load("opengl32.dll")
            .map(|module| LibOpengl32 {
                wglCreateContext: module.get_symbol("wglCreateContext").unwrap(),
                wglDeleteContext: module.get_symbol("wglDeleteContext").unwrap(),
                wglGetProcAddress: module.get_symbol("wglGetProcAddress").unwrap(),
                wglGetCurrentDC: module.get_symbol("wglGetCurrentDC").unwrap(),
                wglMakeCurrent: module.get_symbol("wglMakeCurrent").unwrap(),
                module,
            })
            .ok()
    }
}
