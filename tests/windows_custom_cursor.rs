#![cfg(target_os = "windows")]

use miniquad::{conf::Conf, window, CustomCursor, EventHandler};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use winapi::{
    shared::windef::*,
    um::{wingdi::*, winuser::*},
};

struct Probe {
    hwnd: HWND,
    cursors: [CustomCursor; 2],
    step: usize,
    first_handle: HCURSOR,
    completed: Arc<AtomicBool>,
}

impl Probe {
    unsafe fn selected(&self, hotspot: (u32, u32)) -> HCURSOR {
        // Exercise the actual window procedure without moving the user's mouse.
        assert_eq!(
            SendMessageW(
                self.hwnd,
                WM_SETCURSOR,
                self.hwnd as usize,
                HTCLIENT as isize
            ),
            1
        );
        let handle = GetCursor();
        let mut info: ICONINFO = std::mem::zeroed();
        assert_ne!(GetIconInfo(handle, &mut info), 0);
        assert_eq!(info.fIcon, 0);
        assert_eq!((info.xHotspot, info.yHotspot), hotspot);
        DeleteObject(info.hbmColor as _);
        DeleteObject(info.hbmMask as _);
        handle
    }
}

impl EventHandler for Probe {
    fn update(&mut self) {
        unsafe {
            match self.step {
                0 => window::set_custom_mouse_cursor(&self.cursors[0]).unwrap(),
                1 => {
                    self.first_handle = self.selected((1, 2));
                    // Same prepared image must retain its native handle.
                    window::set_custom_mouse_cursor(&self.cursors[0]).unwrap();
                }
                2 => {
                    assert_eq!(self.selected((1, 2)), self.first_handle);
                    window::set_custom_mouse_cursor(&self.cursors[1]).unwrap();
                }
                3 => {
                    assert_ne!(self.selected((3, 1)), self.first_handle);
                    window::show_mouse(false);
                    window::show_mouse(true);
                }
                4 => {
                    self.selected((3, 1));
                    // Native borders must still use the OS resize cursor.
                    SendMessageW(
                        self.hwnd,
                        WM_SETCURSOR,
                        self.hwnd as usize,
                        HTLEFT as isize | ((WM_MOUSEMOVE as isize) << 16),
                    );
                    assert_eq!(GetCursor(), LoadCursorW(std::ptr::null_mut(), IDC_SIZEWE));
                    window::set_mouse_cursor(miniquad::CursorIcon::Default);
                }
                5 => {
                    SendMessageW(
                        self.hwnd,
                        WM_SETCURSOR,
                        self.hwnd as usize,
                        HTCLIENT as isize,
                    );
                    assert_eq!(GetCursor(), LoadCursorW(std::ptr::null_mut(), IDC_ARROW));
                    // Dropping the source after queuing must not invalidate it.
                    let temporary = CustomCursor::from_rgba(4, 4, &[255; 64], (0, 3)).unwrap();
                    window::set_custom_mouse_cursor(&temporary).unwrap();
                }
                6 => {
                    self.selected((0, 3));
                    self.completed.store(true, Ordering::SeqCst);
                    window::order_quit();
                }
                _ => panic!("cursor probe did not terminate"),
            }
            self.step += 1;
        }
    }
    fn draw(&mut self) {}
}

#[test]
fn native_cursor_switch_reset_visibility_and_window_borders() {
    let completed = Arc::new(AtomicBool::new(false));
    let result = completed.clone();
    miniquad::start(
        Conf {
            window_title: "Native cursor regression test".to_owned(),
            window_width: 160,
            window_height: 120,
            ..Default::default()
        },
        move || unsafe {
            let hwnd = WindowFromDC(wglGetCurrentDC());
            assert!(!hwnd.is_null());
            ShowWindow(hwnd, SW_HIDE);
            Box::new(Probe {
                hwnd,
                cursors: [
                    CustomCursor::from_rgba(4, 4, &[255; 64], (1, 2)).unwrap(),
                    CustomCursor::from_rgba(4, 4, &[128; 64], (3, 1)).unwrap(),
                ],
                step: 0,
                first_handle: std::ptr::null_mut(),
                completed,
            })
        },
    );
    assert!(result.load(Ordering::SeqCst));
}
