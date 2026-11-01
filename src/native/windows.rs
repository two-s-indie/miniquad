use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};

use crate::{
    conf::{Conf, Icon},
    event::{KeyMods, MouseButton},
    native::{NativeDisplayData, Request},
    CursorIcon, EventHandler,
};

use winapi::{
    shared::{
        hidusage::{HID_USAGE_GENERIC_MOUSE, HID_USAGE_PAGE_GENERIC},
        minwindef::{DWORD, HIWORD, LOWORD, LPARAM, LRESULT, TRUE, UINT, WPARAM},
        ntdef::NULL,
        windef::{HBRUSH, HCURSOR, HDC, HICON, HWND, POINT, RECT},
        windowsx::{GET_X_LPARAM, GET_Y_LPARAM},
    },
    um::{
        imm::{ImmGetContext, ImmReleaseContext, HIMC},
        libloaderapi::{GetModuleHandleW, GetProcAddress},
        shellapi::{DragAcceptFiles, DragQueryFileW, HDROP},
        wingdi::*,
        winuser::*,
    },
};

// IME constants
const GCS_RESULTSTR: DWORD = 0x0800;

// IME message constants
const WM_IME_SETCONTEXT: UINT = 0x0281;
const WM_IME_NOTIFY: UINT = 0x0282;
const WM_IME_CHAR: UINT = 0x0286;
const WM_IME_STARTCOMPOSITION: UINT = 0x010D;
const WM_IME_ENDCOMPOSITION: UINT = 0x010E;
const WM_IME_COMPOSITION: UINT = 0x010F;
const WM_INPUTLANGCHANGE: UINT = 0x0051;
const WM_INPUTLANGCHANGEREQUEST: UINT = 0x0050;

// Window focus messages
const WM_SETFOCUS: UINT = 0x0007;
const WM_KILLFOCUS: UINT = 0x0008;

/// Global flag tracking whether IME has been explicitly disabled by the user.
/// This is controlled via `show_keyboard(bool)` API calls.
use std::sync::atomic::AtomicBool;
static IME_USER_DISABLED: AtomicBool = AtomicBool::new(false);

// IME composition form constants
const CFS_POINT: DWORD = 0x0002;
const CFS_CANDIDATEPOS: DWORD = 0x0040;

// ImmAssociateContextEx flags
const IACE_DEFAULT: DWORD = 0x0010;

// COMPOSITIONFORM structure
#[repr(C)]
#[allow(non_snake_case)]
struct COMPOSITIONFORM {
    dwStyle: DWORD,
    ptCurrentPos: POINT,
    rcArea: RECT,
}

// CANDIDATEFORM structure
#[repr(C)]
#[allow(non_snake_case)]
struct CANDIDATEFORM {
    dwIndex: DWORD,
    dwStyle: DWORD,
    ptCurrentPos: POINT,
    rcArea: RECT,
}

// Link to imm32.dll for IME support
#[link(name = "imm32")]
extern "system" {
    fn ImmGetCompositionStringW(
        himc: HIMC,
        index: DWORD,
        buf: *mut std::ffi::c_void,
        len: DWORD,
    ) -> i32;
    fn ImmAssociateContextEx(hwnd: HWND, himc: HIMC, flags: DWORD) -> i32;
    fn ImmAssociateContext(hwnd: HWND, himc: HIMC) -> HIMC;
    fn ImmCreateContext() -> HIMC;
    fn ImmSetCompositionWindow(himc: HIMC, lpCompForm: *const COMPOSITIONFORM) -> i32;
    fn ImmSetCandidateWindow(himc: HIMC, lpCandidate: *const CANDIDATEFORM) -> i32;
    fn ImmGetOpenStatus(himc: HIMC) -> i32;
    fn ImmSetOpenStatus(himc: HIMC, fOpen: i32) -> i32;
}

// IME Association flags
const IACE_CHILDREN: DWORD = 0x0001;

mod clipboard;
mod dpi;
mod events;
mod geometry;
mod keycodes;
mod libopengl32;
mod wgl;

use libopengl32::LibOpengl32;

pub(crate) struct WindowsDisplay {
    fullscreen: bool,
    window_resizable: bool,
    cursor_grabbed: bool,
    iconified: bool,
    window_scale: f32,
    high_dpi: bool,
    window_dpi: u32,
    dpi_api: dpi::DpiApi,
    normal_size: dpi::ClientSize,
    normal_client_size: (i32, i32),
    restore_size_pending: bool,
    windowed_placement: Option<WINDOWPLACEMENT>,
    geometry_depth: usize,
    notification_depth: usize,
    geometry: Option<geometry::Geometry>,
    geometry_suspended: bool,
    notified_size: Option<(i32, i32)>,
    initializing: bool,
    window_destroyed: bool,
    mouse_position_dirty: bool,
    pending_dpi: Option<(u32, RECT)>,
    events: events::PendingEvents,
    mouse_scale: f32,
    show_cursor: bool,
    user_cursor: bool,
    mouse_x: f32,
    mouse_y: f32,
    ime_position: Option<POINT>,
    cursor: HCURSOR,
    libopengl32: LibOpengl32,
    _msg_wnd: HWND,
    msg_dc: HDC,
    wnd: HWND,
    dc: HDC,
    event_handler: Option<Box<dyn EventHandler>>,
    modal_resizing_timer: usize,
    modal_loop_depth: usize,
    update_requested: bool,
}

impl WindowsDisplay {
    unsafe fn set_cursor_grab(this: *mut Self, grab: bool) {
        if (*this).cursor_grabbed && !grab {
            // Downstream clients may have accumulated virtual raw motion while
            // grabbed. Restore absolute client coordinates even without motion.
            (*this).mouse_position_dirty = true;
            (*this).update_requested = true;
        }
        (*this).cursor_grabbed = grab;
        unsafe {
            if grab {
                update_clip_rect((*this).wnd);
            } else {
                ClipCursor(NULL as _);
            }
        }
    }
    unsafe fn show_mouse(this: *mut Self, shown: bool) {
        if (*this).show_cursor != shown {
            (*this).show_cursor = shown;
            unsafe { ShowCursor(shown.into()) };
        }
    }
    /// Set IME candidate window position in client coordinates
    unsafe fn set_ime_position(this: *mut Self, x: i32, y: i32) {
        // Retain the caller's client-pixel anchor even if the IME context is
        // currently unavailable. Composition startup must not replace it.
        (*this).ime_position = Some(POINT { x, y });
        unsafe {
            let himc = ImmGetContext((*this).wnd);
            if himc.is_null() {
                return;
            }

            // Set composition window position (where the composing text appears)
            let comp_form = COMPOSITIONFORM {
                dwStyle: CFS_POINT,
                ptCurrentPos: POINT { x, y },
                rcArea: std::mem::zeroed(),
            };
            ImmSetCompositionWindow(himc, &comp_form);

            // Set candidate window position (the popup with character choices)
            let cand_form = CANDIDATEFORM {
                dwIndex: 0,
                dwStyle: CFS_CANDIDATEPOS,
                ptCurrentPos: POINT { x, y },
                rcArea: std::mem::zeroed(),
            };
            ImmSetCandidateWindow(himc, &cand_form);

            ImmReleaseContext((*this).wnd, himc);
        }
    }

    /// Enable or disable IME for the window.
    /// When disabled, the IME will not process keyboard input, useful for game controls.
    unsafe fn set_ime_enabled(this: *mut Self, enabled: bool) {
        unsafe {
            if enabled {
                IME_USER_DISABLED.store(false, std::sync::atomic::Ordering::Relaxed);
                // Re-associate IME context with the window
                ImmAssociateContextEx((*this).wnd, std::ptr::null_mut(), IACE_DEFAULT);
            } else {
                IME_USER_DISABLED.store(true, std::sync::atomic::Ordering::Relaxed);
                // Disassociate IME context from the window
                ImmAssociateContextEx((*this).wnd, std::ptr::null_mut(), 0);
            }
        }
    }

    unsafe fn set_mouse_cursor(this: *mut Self, cursor_icon: CursorIcon) {
        let cursor_name = match cursor_icon {
            CursorIcon::Default => IDC_ARROW,
            CursorIcon::Help => IDC_HELP,
            CursorIcon::Pointer => IDC_HAND,
            CursorIcon::Wait => IDC_WAIT,
            CursorIcon::Crosshair => IDC_CROSS,
            CursorIcon::Text => IDC_IBEAM,
            CursorIcon::Move => IDC_SIZEALL,
            CursorIcon::NotAllowed => IDC_NO,
            CursorIcon::EWResize => IDC_SIZEWE,
            CursorIcon::NSResize => IDC_SIZENS,
            CursorIcon::NESWResize => IDC_SIZENESW,
            CursorIcon::NWSEResize => IDC_SIZENWSE,
        };
        (*this).cursor = unsafe { LoadCursorW(NULL as _, cursor_name) };
        unsafe { SetCursor((*this).cursor) };

        (*this).user_cursor = cursor_icon != CursorIcon::Default;
    }
}

fn get_win_style(is_fullscreen: bool, is_resizable: bool) -> DWORD {
    if is_fullscreen {
        WS_POPUP | WS_SYSMENU | WS_VISIBLE
    } else {
        let mut win_style: DWORD =
            WS_CLIPSIBLINGS | WS_CLIPCHILDREN | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;

        if is_resizable {
            win_style |= WS_MAXIMIZEBOX | WS_SIZEBOX;
        }

        win_style
    }
}

unsafe fn update_clip_rect(hwnd: HWND) {
    // ClipCursor expects screen coordinates; convert both client-pixel corners.
    let mut rect: RECT = std::mem::zeroed();

    if GetClientRect(hwnd, &mut rect) == 0 || rect.right <= rect.left || rect.bottom <= rect.top {
        return;
    }
    let mut upper_left = POINT {
        x: rect.left,
        y: rect.top,
    };
    let mut lower_right = POINT {
        x: rect.right,
        y: rect.bottom,
    };

    if ClientToScreen(hwnd, &mut upper_left) == 0 || ClientToScreen(hwnd, &mut lower_right) == 0 {
        return;
    }

    SetRect(
        &mut rect as *mut _ as _,
        upper_left.x,
        upper_left.y,
        lower_right.x,
        lower_right.y,
    );
    ClipCursor(&mut rect as *mut _ as _);
}

unsafe fn key_mods() -> KeyMods {
    let mut mods = KeyMods::default();

    if GetKeyState(VK_SHIFT) as u32 & (1u32 << 31) != 0 {
        mods.shift = true;
    }
    if GetKeyState(VK_CONTROL) as u32 & (1u32 << 31) != 0 {
        mods.ctrl = true;
    }
    if GetKeyState(VK_MENU) as u32 & (1u32 << 31) != 0 {
        mods.alt = true;
    }
    if (GetKeyState(VK_LWIN) | GetKeyState(VK_RWIN)) as u32 & (1u32 << 31) != 0 {
        mods.logo = true;
    }

    mods
}

unsafe fn dropped_file_path(hdrop: HDROP, index: UINT) -> Option<PathBuf> {
    // The queried length excludes the terminating NUL, even for long paths.
    let path_len = DragQueryFileW(hdrop, index, std::ptr::null_mut(), 0);
    if path_len == 0 {
        return None;
    }

    let capacity = path_len.checked_add(1)?;
    let mut path_buffer = vec![0u16; capacity as usize];
    let copied = DragQueryFileW(hdrop, index, path_buffer.as_mut_ptr(), capacity);
    if copied != path_len {
        return None;
    }

    Some(PathBuf::from(OsString::from_wide(
        &path_buffer[..copied as usize],
    )))
}

#[cfg(test)]
mod dropped_file_tests {
    use super::dropped_file_path;
    use std::{os::windows::ffi::OsStrExt, path::PathBuf};
    use winapi::{
        shared::{minwindef::BOOL, windef::POINT},
        um::{
            shellapi::{DragFinish, HDROP},
            winbase::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
        },
    };

    // DROPFILES is not exposed by winapi 0.3. The header is followed by a
    // double-NUL-terminated list of UTF-16 paths.
    // https://learn.microsoft.com/windows/win32/api/shlobj_core/ns-shlobj_core-dropfiles
    #[repr(C)]
    struct DropFilesHeader {
        files_offset: u32,
        point: POINT,
        non_client: BOOL,
        wide: BOOL,
    }

    struct DropHandle(HDROP);

    impl DropHandle {
        fn new(paths: &[PathBuf]) -> Self {
            let mut names = Vec::new();
            for path in paths {
                names.extend(path.as_os_str().encode_wide());
                names.push(0);
            }
            names.push(0);
            let header_size = std::mem::size_of::<DropFilesHeader>();

            unsafe {
                let handle = GlobalAlloc(GMEM_MOVEABLE, header_size + names.len() * 2);
                assert!(!handle.is_null());
                let drop = Self(handle as HDROP);
                let pointer = GlobalLock(handle) as *mut u8;
                assert!(!pointer.is_null());
                pointer.cast::<DropFilesHeader>().write(DropFilesHeader {
                    files_offset: header_size as u32,
                    point: POINT { x: 0, y: 0 },
                    non_client: 0,
                    wide: 1,
                });
                std::ptr::copy_nonoverlapping(
                    names.as_ptr(),
                    pointer.add(header_size).cast::<u16>(),
                    names.len(),
                );
                GlobalUnlock(handle);
                drop
            }
        }
    }

    impl Drop for DropHandle {
        fn drop(&mut self) {
            unsafe { DragFinish(self.0) }
        }
    }

    #[test]
    fn paths_at_and_beyond_max_path_are_not_truncated() {
        let prefix = format!("C:\\{}", "directory\\".repeat(20));
        let paths: Vec<PathBuf> = [259, 260, 261, 1024]
            .iter()
            .map(|&len| {
                let directories = "subdir\\".repeat((len - prefix.len() - 4) / 7);
                let tail_len = len - prefix.len() - directories.len() - 4;
                PathBuf::from(format!(
                    "{}{}{}.txt",
                    prefix,
                    directories,
                    "a".repeat(tail_len)
                ))
            })
            .collect();
        let drop = DropHandle::new(&paths);

        for (index, path) in paths.into_iter().enumerate() {
            assert_eq!(
                unsafe { dropped_file_path(drop.0, index as u32) },
                Some(path)
            );
        }
        assert_eq!(unsafe { dropped_file_path(drop.0, 4) }, None);
    }

    #[test]
    fn mixed_short_and_long_unicode_paths_are_preserved() {
        let paths = [
            PathBuf::from("C:\\short.txt"),
            PathBuf::from(format!(
                "C:\\{}report-\u{1f980}.txt",
                "\u{8cc7}\u{6599}\\".repeat(100)
            )),
            PathBuf::from("C:\\last.txt"),
        ];
        let drop = DropHandle::new(&paths);

        for (index, path) in paths.iter().enumerate() {
            assert_eq!(
                unsafe { dropped_file_path(drop.0, index as u32) }.as_ref(),
                Some(path)
            );
        }
    }
}

unsafe extern "system" fn win32_wndproc(
    hwnd: HWND,
    umsg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    #[cfg(target_pointer_width = "64")]
    let display_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    #[cfg(target_pointer_width = "32")]
    let display_ptr = GetWindowLongW(hwnd, GWLP_USERDATA) as isize;

    if display_ptr == 0 {
        return DefWindowProcW(hwnd, umsg, wparam, lparam);
    }
    let payload = display_ptr as *mut WindowsDisplay;
    if umsg == WM_NCDESTROY {
        WindowsDisplay::detach(payload);
        (*payload).window_destroyed = true;
        crate::native_display().lock().unwrap().quit_ordered = true;
        PostQuitMessage(0);
        return DefWindowProcW(hwnd, umsg, wparam, lparam);
    }
    // DefWindowProc(WM_WINDOWPOSCHANGED) nests WM_SIZE/WM_MOVE. Deliver their
    // callbacks together after it finishes, not during non-client calculation.
    let preparing_geometry = matches!(
        umsg,
        WM_WINDOWPOSCHANGING | WM_NCCALCSIZE | WM_GETMINMAXINFO
    );
    let geometry_message = preparing_geometry || umsg == WM_WINDOWPOSCHANGED;
    if geometry_message {
        (*payload).notification_depth += 1;
    }
    let result = window_message(payload, hwnd, umsg, wparam, lparam);
    if geometry_message {
        (*payload).notification_depth -= 1;
    }
    if !preparing_geometry {
        WindowsDisplay::flush_events(payload);
    }
    result
}

// Keep all display borrows local to a field access. DefWindowProc, IME calls,
// SetWindowPos and application callbacks can synchronously re-enter this code.
unsafe fn window_message(
    payload: *mut WindowsDisplay,
    hwnd: HWND,
    umsg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match umsg {
        WM_CLOSE => {
            (*payload).events.quit_requested_event();
            return 0;
        }
        WM_DPICHANGED => {
            let dpi = LOWORD(wparam as _) as u32;
            if dpi != 0 && lparam != 0 {
                // lParam belongs to Windows and is valid only during this call.
                let suggested = *(lparam as *const RECT);
                WindowsDisplay::change_dpi(payload, dpi, suggested);
            }
            return 0;
        }
        WM_SYSCOMMAND => {
            match wparam & 0xFFF0 {
                SC_SCREENSAVE | SC_MONITORPOWER => {
                    if (*payload).fullscreen {
                        // disable screen saver and blanking in fullscreen mode
                        return 0;
                    }
                }
                SC_KEYMENU => {
                    // user trying to access menu via ALT
                    return 0;
                }
                _ => {}
            }
        }
        WM_ERASEBKGND => {
            return 1;
        }
        WM_SIZE => {
            WindowsDisplay::size_changed(payload);
            let iconified = wparam == SIZE_MINIMIZED;
            if iconified != (*payload).iconified {
                (*payload).iconified = iconified;
                if iconified {
                    (*payload).events.window_minimized_event();
                } else {
                    (*payload).events.window_restored_event();
                }
            }
        }
        WM_SETCURSOR => {
            if (*payload).user_cursor && LOWORD(lparam as _) == HTCLIENT as _ {
                SetCursor((*payload).cursor);

                return 1;
            }
        }
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONUP
        | WM_MBUTTONUP => {
            // Button messages carry their own signed client coordinates. A
            // preceding WM_MOUSEMOVE is not guaranteed after geometry changes.
            let x = GET_X_LPARAM(lparam) as f32 * (*payload).mouse_scale;
            let y = GET_Y_LPARAM(lparam) as f32 * (*payload).mouse_scale;
            (*payload).mouse_x = x;
            (*payload).mouse_y = y;
            let button = match umsg {
                WM_LBUTTONDOWN | WM_LBUTTONUP => MouseButton::Left,
                WM_RBUTTONDOWN | WM_RBUTTONUP => MouseButton::Right,
                _ => MouseButton::Middle,
            };
            if matches!(umsg, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN) {
                (*payload).events.mouse_button_down_event(button, x, y);
            } else {
                (*payload).events.mouse_button_up_event(button, x, y);
            }
        }

        WM_MOUSEMOVE => {
            (*payload).mouse_x = GET_X_LPARAM(lparam) as f32 * (*payload).mouse_scale;
            (*payload).mouse_y = GET_Y_LPARAM(lparam) as f32 * (*payload).mouse_scale;
            // mouse enter was not handled by miniquad anyway
            // if !_sapp.win32_mouse_tracked {
            //     _sapp.win32_mouse_tracked = true;

            //     let mut tme: TRACKMOUSEEVENT = std::mem::zeroed();

            //     tme.cbSize = std::mem::size_of_val(&tme) as _;
            //     tme.dwFlags = TME_LEAVE;
            //     tme.hwndTrack = wnd;
            //     TrackMouseEvent(&mut tme as *mut _);
            //     _sapp_win32_mouse_event(
            //         sapp_event_type_SAPP_EVENTTYPE_MOUSE_ENTER,
            //         sapp_mousebutton_SAPP_MOUSEBUTTON_INVALID,
            //     );
            // }

            let mouse_x = (*payload).mouse_x;
            let mouse_y = (*payload).mouse_y;

            (*payload).events.mouse_motion_event(mouse_x, mouse_y);
        }

        WM_MOVE if (*payload).cursor_grabbed => {
            update_clip_rect(hwnd);
        }

        WM_INPUT => {
            let mut data: RAWINPUT = std::mem::zeroed();
            let mut size = std::mem::size_of::<RAWINPUT>();
            let get_succeed = GetRawInputData(
                lparam as _,
                RID_INPUT,
                &mut data as *mut _ as _,
                &mut size as *mut _ as _,
                std::mem::size_of::<RAWINPUTHEADER>() as _,
            );
            if get_succeed as i32 == -1 {
                panic!("failed to retrieve raw input data");
            }

            let mouse_scale = (*payload).mouse_scale;
            let mut dx = data.data.mouse().lLastX as f32 * mouse_scale;
            let mut dy = data.data.mouse().lLastY as f32 * mouse_scale;

            // convert from normalised absolute coordinates
            if (data.data.mouse().usFlags & MOUSE_MOVE_ABSOLUTE) == MOUSE_MOVE_ABSOLUTE {
                let (width, height) = {
                    let d = crate::native_display().lock().unwrap();
                    (d.screen_width as f32, d.screen_height as f32)
                };

                dx = dx / 65535.0 * width;
                dy = dy / 65535.0 * height;
            }

            (*payload).events.raw_mouse_motion(dx, dy);
        }

        WM_MOUSELEAVE => {
            // mouse leave was not handled by miniquad anyway
            // _sapp.win32_mouse_tracked = false;
            // _sapp_win32_mouse_event(
            //     sapp_event_type_SAPP_EVENTTYPE_MOUSE_LEAVE,
            //     sapp_mousebutton_SAPP_MOUSEBUTTON_INVALID,
            // );
        }
        WM_MOUSEWHEEL => {
            (*payload)
                .events
                .mouse_wheel_event(0.0, (HIWORD(wparam as _) as i16) as f32);
        }

        WM_MOUSEHWHEEL => {
            (*payload)
                .events
                .mouse_wheel_event((HIWORD(wparam as _) as i16) as f32, 0.0);
        }
        WM_CHAR => {
            let chr = wparam as u32;
            let repeat = !!(lparam & 0x40000000) != 0;
            let mods = key_mods();
            if chr > 0 {
                if let Some(chr) = char::from_u32(chr) {
                    (*payload).events.char_event(chr, mods, repeat);
                }
            }
            return 0;
        }
        // IME message handling:
        // We manually extract the result string from WM_IME_COMPOSITION
        // instead of relying on WM_IME_CHAR to avoid duplicate characters.
        WM_IME_CHAR => {
            // Already handled in WM_IME_COMPOSITION; ignore to prevent duplicates
            return 0;
        }
        WM_IME_COMPOSITION => {
            let flags = lparam as u32;

            // Extract and dispatch the result string manually to avoid duplicates
            if (flags & GCS_RESULTSTR) != 0 {
                let himc = ImmGetContext(hwnd);
                if !himc.is_null() {
                    let len =
                        ImmGetCompositionStringW(himc, GCS_RESULTSTR, std::ptr::null_mut(), 0);
                    if len > 0 {
                        let mut buffer: Vec<u16> = vec![0; (len as usize / 2) + 1];
                        let actual_len = ImmGetCompositionStringW(
                            himc,
                            GCS_RESULTSTR,
                            buffer.as_mut_ptr() as *mut _,
                            len as u32,
                        );
                        if actual_len > 0 {
                            let char_count = actual_len as usize / 2;
                            let mods = key_mods();
                            // Send chars in order
                            for i in 0..char_count {
                                let chr = buffer[i];
                                if let Some(c) = char::from_u32(chr as u32) {
                                    (*payload).events.char_event(c, mods, false);
                                }
                            }
                        }
                    }
                    ImmReleaseContext(hwnd, himc);
                }
                return 0;
            }

            // For non-result messages (composition state updates), pass to DefWindowProc
            return DefWindowProcW(hwnd, umsg, wparam, lparam);
        }
        WM_IME_SETCONTEXT => {
            let user_disabled = IME_USER_DISABLED.load(std::sync::atomic::Ordering::Relaxed);

            // If user explicitly disabled IME, don't auto-restore
            if user_disabled {
                return 0;
            }

            // Must pass to DefWindowProc to enable IME properly
            return DefWindowProcW(hwnd, umsg, wparam, lparam);
        }
        WM_IME_STARTCOMPOSITION => {
            if let Some(position) = (*payload).ime_position {
                WindowsDisplay::set_ime_position(payload, position.x, position.y);
                return DefWindowProcW(hwnd, umsg, wparam, lparam);
            }
            // Offset for candidate window below composition position
            const CANDIDATE_WINDOW_Y_OFFSET: i32 = 20;

            // Set candidate window position when IME starts composition
            let himc = ImmGetContext(hwnd);
            if !himc.is_null() {
                let mut pt: POINT = std::mem::zeroed();
                GetCaretPos(&mut pt);

                let comp_form = COMPOSITIONFORM {
                    dwStyle: CFS_POINT,
                    ptCurrentPos: pt,
                    rcArea: RECT {
                        left: 0,
                        top: 0,
                        right: 0,
                        bottom: 0,
                    },
                };
                ImmSetCompositionWindow(himc, &comp_form);

                // Set candidate window position (most IMEs only use index 0)
                let cand_form = CANDIDATEFORM {
                    dwIndex: 0,
                    dwStyle: CFS_CANDIDATEPOS,
                    ptCurrentPos: POINT {
                        x: pt.x,
                        y: pt.y + CANDIDATE_WINDOW_Y_OFFSET,
                    },
                    rcArea: RECT {
                        left: 0,
                        top: 0,
                        right: 0,
                        bottom: 0,
                    },
                };
                ImmSetCandidateWindow(himc, &cand_form);

                ImmReleaseContext(hwnd, himc);
            }
            return DefWindowProcW(hwnd, umsg, wparam, lparam);
        }
        WM_IME_ENDCOMPOSITION => {
            return DefWindowProcW(hwnd, umsg, wparam, lparam);
        }
        WM_IME_NOTIFY => {
            const IMN_SETOPENSTATUS: WPARAM = 0x0008;

            // Re-enable IME if it was unexpectedly closed (unless user disabled it)
            if wparam == IMN_SETOPENSTATUS {
                let himc = ImmGetContext(hwnd);
                if !himc.is_null() {
                    let open_status = ImmGetOpenStatus(himc);
                    let user_disabled =
                        IME_USER_DISABLED.load(std::sync::atomic::Ordering::Relaxed);
                    if open_status == 0 && !user_disabled {
                        ImmSetOpenStatus(himc, 1);
                    }
                    ImmReleaseContext(hwnd, himc);
                }
            }

            return DefWindowProcW(hwnd, umsg, wparam, lparam);
        }
        WM_INPUTLANGCHANGEREQUEST | WM_INPUTLANGCHANGE => {
            // Pass input language change messages to default handler
            return DefWindowProcW(hwnd, umsg, wparam, lparam);
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let keycode_raw = HIWORD(lparam as _) as u32 & 0x1FF;
            let keycode = keycodes::translate_keycode(keycode_raw);
            let mods = key_mods();
            let repeat = !!(lparam & 0x40000000) != 0;
            (*payload).events.key_down_event(keycode, mods, repeat);
            // Pass to DefWindowProc for IME to work properly
            return DefWindowProcW(hwnd, umsg, wparam, lparam);
        }
        WM_KEYUP | WM_SYSKEYUP => {
            let keycode = HIWORD(lparam as _) as u32 & 0x1FF;
            let keycode = keycodes::translate_keycode(keycode);
            let mods = key_mods();
            (*payload).events.key_up_event(keycode, mods);
            // IMPORTANT: Pass to DefWindowProc for IME to work properly
            return DefWindowProcW(hwnd, umsg, wparam, lparam);
        }
        WM_ENTERSIZEMOVE | WM_ENTERMENULOOP => {
            if (*payload).modal_loop_depth == 0 {
                (*payload).modal_resizing_timer = SetTimer(
                    hwnd,
                    std::ptr::addr_of!((*payload).modal_resizing_timer) as usize,
                    10,
                    None,
                );
            }
            (*payload).modal_loop_depth += 1;
        }
        WM_TIMER => {
            if (*payload).modal_loop_depth != 0
                && wparam == (*payload).modal_resizing_timer
                && wparam != 0
            {
                WindowsDisplay::frame(payload, true);
            }
        }
        WM_EXITSIZEMOVE | WM_EXITMENULOOP => {
            (*payload).modal_loop_depth = (*payload).modal_loop_depth.saturating_sub(1);
            if (*payload).modal_loop_depth == 0 {
                WindowsDisplay::stop_modal_timer(payload);
            }
        }
        WM_DROPFILES => {
            let hdrop = wparam as HDROP;
            let num_drops = DragQueryFileW(hdrop, u32::MAX, std::ptr::null_mut(), 0);

            let mut d = crate::native_display().lock().unwrap();
            d.dropped_files = Default::default();
            for i in 0..num_drops {
                if let Some(path) = dropped_file_path(hdrop, i) {
                    if let Ok(bytes) = std::fs::read(&path) {
                        d.dropped_files.bytes.push(bytes);
                        d.dropped_files.paths.push(path);
                    }
                }
            }
        }
        WM_ACTIVATE => {
            if LOWORD(wparam as _) == WA_ACTIVE || LOWORD(wparam as _) == WA_CLICKACTIVE {
                (*payload).events.window_restored_event();
            } else {
                (*payload).events.window_minimized_event();
            }
        }
        WM_SETFOCUS => {
            let user_disabled = IME_USER_DISABLED.load(std::sync::atomic::Ordering::Relaxed);

            // Ensure IME context is available when window gains focus
            if !user_disabled {
                let himc = ImmGetContext(hwnd);

                if himc.is_null() {
                    // Create new IME context if none exists
                    let new_himc = ImmCreateContext();
                    if !new_himc.is_null() {
                        let prev_himc = ImmAssociateContext(hwnd, new_himc);
                        if ImmSetOpenStatus(new_himc, 1) == 0 && prev_himc.is_null() {
                            // If SetOpenStatus fails and no previous context, release the new one
                            ImmReleaseContext(hwnd, new_himc);
                        }
                    }
                } else {
                    // Ensure IME is open
                    let open_status = ImmGetOpenStatus(himc);
                    if open_status == 0 {
                        ImmSetOpenStatus(himc, 1);
                    }
                    ImmReleaseContext(hwnd, himc);
                }
            }

            return DefWindowProcW(hwnd, umsg, wparam, lparam);
        }
        WM_KILLFOCUS => {
            return DefWindowProcW(hwnd, umsg, wparam, lparam);
        }
        _ => {}
    }

    DefWindowProcW(hwnd, umsg, wparam, lparam)
}

unsafe fn create_win_icon_from_image(width: u32, height: u32, colors: &[u8]) -> Option<HICON> {
    let mut bi: BITMAPV5HEADER = std::mem::zeroed();

    bi.bV5Size = std::mem::size_of::<BITMAPV5HEADER>() as _;
    bi.bV5Width = width as i32;
    bi.bV5Height = -(height as i32); // NOTE the '-' here to indicate that origin is top-left
    bi.bV5Planes = 1;
    bi.bV5BitCount = 32;
    bi.bV5Compression = BI_BITFIELDS;
    bi.bV5RedMask = 0x00FF0000;
    bi.bV5GreenMask = 0x0000FF00;
    bi.bV5BlueMask = 0x000000FF;
    bi.bV5AlphaMask = 0xFF000000;

    let mut target = std::ptr::null_mut();
    // const uint8_t* source = (const uint8_t*)desc->pixels.ptr;

    let dc = GetDC(std::ptr::null_mut());
    let color = CreateDIBSection(
        dc,
        &bi as *const _ as *const BITMAPINFO,
        DIB_RGB_COLORS,
        &mut target,
        std::ptr::null_mut(),
        0,
    );
    ReleaseDC(std::ptr::null_mut(), dc);
    if color.is_null() {
        return None;
    }
    assert!(!target.is_null());

    let mask = CreateBitmap(width as _, height as _, 1, 1, std::ptr::null());
    if mask.is_null() {
        DeleteObject(color as *mut _);
        return None;
    }

    for i in 0..width as usize * height as usize {
        *(target as *mut u8).offset(i as isize * 4 + 0) = colors[i * 4 + 2];
        *(target as *mut u8).offset(i as isize * 4 + 1) = colors[i * 4 + 1];
        *(target as *mut u8).offset(i as isize * 4 + 2) = colors[i * 4 + 0];
        *(target as *mut u8).offset(i as isize * 4 + 3) = colors[i * 4 + 3];
    }

    let mut icon_info: ICONINFO = std::mem::zeroed();
    icon_info.fIcon = 1;
    icon_info.xHotspot = 0;
    icon_info.yHotspot = 0;
    icon_info.hbmMask = mask;
    icon_info.hbmColor = color;
    let icon_handle = CreateIconIndirect(&mut icon_info);
    DeleteObject(color as *mut _);
    DeleteObject(mask as *mut _);

    Some(icon_handle)
}

unsafe fn set_icon(wnd: HWND, icon: &Icon) {
    let big_icon_w = GetSystemMetrics(SM_CXICON);
    let big_icon_h = GetSystemMetrics(SM_CYICON);
    let small_icon_w = GetSystemMetrics(SM_CXSMICON);
    let small_icon_h = GetSystemMetrics(SM_CYSMICON);

    let big_icon = if big_icon_w * big_icon_h >= 64 * 64 {
        (&icon.big[..], 64, 64)
    } else {
        (&icon.medium[..], 32, 32)
    };

    let small_icon = if small_icon_w * small_icon_h <= 16 * 16 {
        (&icon.small[..], 16, 16)
    } else {
        (&icon.medium[..], 32, 32)
    };

    let big_icon = create_win_icon_from_image(big_icon.1, big_icon.2, big_icon.0);
    let small_icon = create_win_icon_from_image(small_icon.1, small_icon.2, small_icon.0);
    if let Some(icon) = big_icon {
        SendMessageW(wnd, WM_SETICON, ICON_BIG as _, icon as LPARAM);
    }
    if let Some(icon) = small_icon {
        SendMessageW(wnd, WM_SETICON, ICON_SMALL as _, icon as LPARAM);
    }
}

unsafe fn create_window(conf: &Conf, dpi_api: &dpi::DpiApi) -> (HWND, HDC, u32) {
    let mut wndclassw: WNDCLASSW = std::mem::zeroed();

    // CS_OWNDC is required for OpenGL
    wndclassw.style = CS_HREDRAW | CS_VREDRAW | CS_OWNDC;
    wndclassw.lpfnWndProc = Some(win32_wndproc);
    wndclassw.hInstance = GetModuleHandleW(NULL as _);
    wndclassw.hCursor = LoadCursorW(NULL as _, IDC_ARROW);
    wndclassw.hIcon = LoadIconW(NULL as _, IDI_WINLOGO);
    wndclassw.hbrBackground = GetStockObject(BLACK_BRUSH as i32) as HBRUSH;
    let class_name = "MINIQUADAPP\0".encode_utf16().collect::<Vec<u16>>();
    wndclassw.lpszClassName = class_name.as_ptr() as _;
    wndclassw.cbWndExtra = std::mem::size_of::<*mut std::ffi::c_void>() as i32;
    RegisterClassW(&wndclassw);

    let win_style: DWORD;
    let win_ex_style: DWORD = WS_EX_APPWINDOW | WS_EX_WINDOWEDGE;
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };

    if conf.fullscreen {
        // Even fullscreen startup must stay hidden until WGL is initialized.
        win_style = WS_POPUP | WS_SYSMENU;
        rect.right = GetSystemMetrics(SM_CXSCREEN);
        rect.bottom = GetSystemMetrics(SM_CYSCREEN);
    } else {
        win_style = if conf.window_resizable {
            WS_CLIPSIBLINGS
                | WS_CLIPCHILDREN
                | WS_CAPTION
                | WS_SYSMENU
                | WS_MINIMIZEBOX
                | WS_MAXIMIZEBOX
                | WS_SIZEBOX
        } else {
            WS_CLIPSIBLINGS | WS_CLIPCHILDREN | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX
        };

        rect.right = dpi::startup_pixels(conf.window_width, dpi::BASE_DPI)
            .expect("Invalid Windows startup width");
        rect.bottom = dpi::startup_pixels(conf.window_height, dpi::BASE_DPI)
            .expect("Invalid Windows startup height");
    }

    let client_rect = rect;
    if AdjustWindowRectEx(&mut rect, win_style, false as _, win_ex_style) == 0 {
        rect = client_rect;
    }
    let win_width = rect
        .right
        .checked_sub(rect.left)
        .filter(|&v| v > 0)
        .expect("Invalid Windows outer width");
    let win_height = rect
        .bottom
        .checked_sub(rect.top)
        .filter(|&v| v > 0)
        .expect("Invalid Windows outer height");
    let class_name = "MINIQUADAPP\0".encode_utf16().collect::<Vec<u16>>();
    let mut window_name = conf.window_title.encode_utf16().collect::<Vec<u16>>();
    window_name.push(0);
    let hwnd = CreateWindowExW(
        win_ex_style,                // dwExStyle
        class_name.as_ptr(),         // lpClassName
        window_name.as_ptr(),        // lpWindowName
        win_style,                   // dwStyle
        CW_USEDEFAULT,               // X
        CW_USEDEFAULT,               // Y
        win_width,                   // nWidth
        win_height,                  // nHeight
        NULL as _,                   // hWndParent
        NULL as _,                   // hMenu
        GetModuleHandleW(NULL as _), // hInstance
        NULL as _,                   // lparam
    );
    assert!(!hwnd.is_null());

    // NOTE: Do not call ShowWindow here!
    // Must show window AFTER SetPixelFormat and wglCreateContext
    // Otherwise IME will not work correctly.
    let dc = GetDC(hwnd);
    assert!(!dc.is_null());

    let mut window_dpi = dpi_api.window_dpi(hwnd, dc);
    if !conf.fullscreen {
        // A larger window can overlap a different monitor. Requery after sizing,
        // always deriving from the original request rather than a rounded result.
        // Bound retries so OS constraints cannot cause a startup resize loop.
        for _ in 0..3 {
            let render_dpi = if conf.high_dpi {
                window_dpi
            } else {
                dpi::BASE_DPI
            };
            let size = (
                dpi::startup_pixels(conf.window_width, render_dpi)
                    .expect("Invalid Windows startup width"),
                dpi::startup_pixels(conf.window_height, render_dpi)
                    .expect("Invalid Windows startup height"),
            );
            let resized = dpi_api.resize_client(hwnd, size, win_style, win_ex_style, window_dpi);
            let actual_dpi = dpi_api.window_dpi(hwnd, dc);
            if actual_dpi == window_dpi || !resized {
                window_dpi = actual_dpi;
                break;
            }
            window_dpi = actual_dpi;
        }
    }
    DragAcceptFiles(hwnd, TRUE);

    (hwnd, dc, window_dpi)
}

unsafe fn create_msg_window() -> (HWND, HDC) {
    // Use a separate window class to avoid interfering with main window's IME
    let class_name = "MINIQUADMSGWND\0".encode_utf16().collect::<Vec<u16>>();

    let mut wndclassw: WNDCLASSW = std::mem::zeroed();
    wndclassw.style = 0;
    wndclassw.lpfnWndProc = Some(DefWindowProcW);
    wndclassw.hInstance = GetModuleHandleW(NULL as _);
    wndclassw.lpszClassName = class_name.as_ptr() as _;
    RegisterClassW(&wndclassw);

    let window_name = "miniquad message window\0"
        .encode_utf16()
        .collect::<Vec<u16>>();
    let msg_hwnd = CreateWindowExW(
        WS_EX_OVERLAPPEDWINDOW,
        class_name.as_ptr() as _,
        window_name.as_ptr() as _,
        WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
        0,
        0,
        1,
        1,
        NULL as _,
        NULL as _,
        GetModuleHandleW(NULL as _),
        NULL,
    );
    assert!(
        !msg_hwnd.is_null(),
        "Win32: failed to create helper window!"
    );

    // Disable IME for message window to avoid interfering with main window
    ImmAssociateContextEx(msg_hwnd, std::ptr::null_mut(), IACE_CHILDREN);

    ShowWindow(msg_hwnd, SW_HIDE);
    let mut msg = std::mem::zeroed();
    while PeekMessageW(&mut msg as _, msg_hwnd, 0, 0, PM_REMOVE) != 0 {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    let msg_dc = GetDC(msg_hwnd);
    assert!(
        !msg_dc.is_null(),
        "Win32: failed to obtain helper window DC!"
    );

    (msg_hwnd, msg_dc)
}

impl WindowsDisplay {
    unsafe fn get_proc_address(&mut self, proc: &str) -> Option<unsafe extern "C" fn() -> ()> {
        let proc = std::ffi::CString::new(proc).unwrap();
        let mut proc_ptr = (self.libopengl32.wglGetProcAddress)(proc.as_ptr());
        if proc_ptr.is_null() {
            proc_ptr = GetProcAddress(self.libopengl32.module.0, proc.as_ptr());
        }
        if proc_ptr.is_null() {
            eprintln!("Load GL func {:?} failed.", proc);
            return None;
        }
        Some(std::mem::transmute::<
            *mut winapi::shared::minwindef::__some_function,
            unsafe extern "C" fn(),
        >(proc_ptr))
    }
}

pub fn run<F>(conf: &Conf, f: F)
where
    F: 'static + FnOnce() -> Box<dyn EventHandler>,
{
    unsafe {
        let dpi_api = dpi::DpiApi::load();
        dpi_api.prepare_process(conf.high_dpi);
        let (wnd, dc, window_dpi) = create_window(conf, &dpi_api);
        if let Some(icon) = &conf.icon {
            set_icon(wnd, icon);
        }

        let libopengl32 = LibOpengl32::try_load().expect("Failed to load opengl32.dll.");

        let (msg_wnd, msg_dc) = create_msg_window();
        let mut display = WindowsDisplay {
            fullscreen: conf.fullscreen,
            window_resizable: conf.window_resizable,
            cursor_grabbed: false,
            iconified: false,
            high_dpi: conf.high_dpi,
            window_dpi,
            dpi_api,
            normal_size: dpi::ClientSize::new(
                (conf.window_width, conf.window_height),
                dpi::BASE_DPI,
            ),
            normal_client_size: (0, 0),
            restore_size_pending: false,
            windowed_placement: None,
            geometry_depth: 0,
            notification_depth: 0,
            geometry: None,
            geometry_suspended: false,
            notified_size: None,
            initializing: true,
            window_destroyed: false,
            mouse_position_dirty: false,
            pending_dpi: None,
            events: Default::default(),
            mouse_scale: 1.,
            window_scale: if conf.high_dpi {
                window_dpi as f32 / dpi::BASE_DPI as f32
            } else {
                1.0
            },
            mouse_x: 0.,
            mouse_y: 0.,
            ime_position: None,
            show_cursor: true,
            user_cursor: false,
            cursor: std::ptr::null_mut(),
            libopengl32,
            _msg_wnd: msg_wnd,
            msg_dc,
            wnd,
            dc,
            event_handler: None,
            modal_resizing_timer: 0,
            modal_loop_depth: 0,
            update_requested: true,
        };

        let (tx, rx) = std::sync::mpsc::channel();
        let clipboard = Box::new(clipboard::WindowsClipboard::new());
        crate::set_display(NativeDisplayData {
            high_dpi: conf.high_dpi,
            dpi_scale: display.window_scale,
            blocking_event_loop: conf.platform.blocking_event_loop,
            ..NativeDisplayData::new(conf.window_width, conf.window_height, tx, clipboard)
        });

        WindowsDisplay::publish_geometry(&mut display);

        let mut wgl = wgl::Wgl::new(&mut display);
        let gl_ctx = wgl.create_context(
            &mut display,
            conf.sample_count,
            conf.platform.swap_interval.unwrap_or(1),
        );

        super::gl::load_gl_funcs(|proc| display.get_proc_address(proc));

        // Install the stable payload once shared state and WGL exist, before
        // showing the window or running user initialization (both may send DPI
        // and geometry messages). No handler is borrowed across either call.
        let display = &mut display as *mut WindowsDisplay;
        #[cfg(target_pointer_width = "64")]
        SetWindowLongPtrW(wnd, GWLP_USERDATA, display as isize);
        #[cfg(target_pointer_width = "32")]
        SetWindowLongW(wnd, GWLP_USERDATA, display as isize);

        // IMPORTANT: Show window AFTER WGL context is created
        // This ensures IME initializes correctly when window gains focus
        // Showing/host startup placement is not an intentional user resize;
        // do not replace logical intent with the rounded startup pixels.
        (*display).geometry_depth += 1;
        ShowWindow(wnd, SW_SHOW);
        (*display).geometry_depth -= 1;
        // ShowWindow can apply startup placement/minimization supplied by a host.
        // Make the initial application callback observe the resulting client size.
        WindowsDisplay::flush_events(display);
        (*display).notified_size = (*display).geometry.map(|geometry| geometry.size);

        // Register for raw mouse input (needed for raw_mouse_motion event)
        let mut client: RECT = std::mem::zeroed();
        if GetClientRect(wnd, &mut client) != 0 {
            (*display).normal_client_size =
                (client.right - client.left, client.bottom - client.top);
        }
        let mut rawinputdevice: RAWINPUTDEVICE = std::mem::zeroed();
        rawinputdevice.usUsagePage = HID_USAGE_PAGE_GENERIC;
        rawinputdevice.usUsage = HID_USAGE_GENERIC_MOUSE;
        rawinputdevice.hwndTarget = NULL as _;
        let register_succeed = RegisterRawInputDevices(
            &rawinputdevice as *const _,
            1,
            std::mem::size_of::<RAWINPUTDEVICE>() as _,
        );
        assert!(
            register_succeed == 1,
            "Win32: failed to register for raw mouse input!"
        );

        let handler = f();
        (*display).event_handler = Some(handler);
        (*display).initializing = false;
        WindowsDisplay::flush_events(display);

        let mut done = false;
        while !(done || crate::native_display().lock().unwrap().quit_ordered) {
            while let Ok(request) = rx.try_recv() {
                WindowsDisplay::process_request(display, request);
            }

            if crate::native_display().lock().unwrap().quit_ordered {
                break;
            }
            let mut msg: MSG = std::mem::zeroed();
            let block_on_wait = conf.platform.blocking_event_loop && !(*display).update_requested;
            if block_on_wait {
                let result = GetMessageW(&mut msg, NULL as _, 0, 0);
                if result <= 0 {
                    if result == -1 {
                        eprintln!(
                            "Win32 GetMessage failed: {}",
                            std::io::Error::last_os_error()
                        );
                    }
                    break;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            } else {
                while PeekMessageW(&mut msg as *mut _ as _, NULL as _, 0, 0, PM_REMOVE) != 0 {
                    if msg.message == WM_QUIT {
                        done = true;
                        break;
                    }
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                    if crate::native_display().lock().unwrap().quit_ordered {
                        break;
                    }
                }
            }

            if done || crate::native_display().lock().unwrap().quit_ordered {
                break;
            }
            WindowsDisplay::frame(display, !conf.platform.blocking_event_loop);
            if crate::native_display().lock().unwrap().quit_requested {
                PostMessageW((*display).wnd, WM_CLOSE, 0, 0);
            }
        }

        // DestroyWindow sends focus/size messages. Detach first so shutdown
        // cannot run application callbacks after the graphics context is gone.
        if !(*display).window_destroyed {
            WindowsDisplay::detach(display);
        }
        ((*display).libopengl32.wglDeleteContext)(gl_ctx);
        if !(*display).window_destroyed {
            DestroyWindow(wnd);
        }
    }
}
