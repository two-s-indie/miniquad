//! Optional Windows DPI APIs. Keep newer entry points out of the import table.

use crate::native::module::Module;
use std::convert::TryFrom;
use winapi::{
    shared::{
        minwindef::{BOOL, DWORD, UINT},
        windef::{
            DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE,
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, HDC, HMONITOR, HWND, RECT,
        },
        winerror::ERROR_INVALID_PARAMETER,
    },
    um::{
        errhandlingapi::GetLastError,
        libloaderapi::{LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32},
        shellscalingapi::{
            MDT_EFFECTIVE_DPI, MONITOR_DPI_TYPE, PROCESS_DPI_AWARENESS,
            PROCESS_PER_MONITOR_DPI_AWARE,
        },
        wingdi::{GetDeviceCaps, LOGPIXELSX},
        winnt::HRESULT,
        winuser::{
            AdjustWindowRectEx, GetClientRect, GetWindowRect, MonitorFromWindow,
            SetProcessDPIAware, SetWindowPos, MONITOR_DEFAULTTONEAREST, SWP_NOACTIVATE, SWP_NOMOVE,
            SWP_NOZORDER,
        },
    },
};

pub(super) const BASE_DPI: u32 = 96;

type SetProcessContext = unsafe extern "system" fn(DPI_AWARENESS_CONTEXT) -> BOOL;
type SetProcessAwareness = unsafe extern "system" fn(PROCESS_DPI_AWARENESS) -> HRESULT;
type GetWindowDpi = unsafe extern "system" fn(HWND) -> UINT;
type GetMonitorDpi =
    unsafe extern "system" fn(HMONITOR, MONITOR_DPI_TYPE, *mut UINT, *mut UINT) -> HRESULT;
type AdjustForDpi = unsafe extern "system" fn(*mut RECT, DWORD, BOOL, DWORD, UINT) -> BOOL;

#[derive(Default)]
pub(super) struct DpiApi {
    set_process_context: Option<SetProcessContext>,
    set_process_awareness: Option<SetProcessAwareness>,
    get_window_dpi: Option<GetWindowDpi>,
    get_monitor_dpi: Option<GetMonitorDpi>,
    adjust_for_dpi: Option<AdjustForDpi>,
    // Function pointers remain valid for the lifetime of these modules.
    _user32: Option<Module>,
    _shcore: Option<Module>,
}

impl DpiApi {
    pub(super) fn load() -> Self {
        let user32 = Module::load("user32.dll").ok();
        // Optional DLL: load only from the system directory. Older Windows may
        // reject the search flag or lack shcore; either case uses the legacy path.
        let name: Vec<u16> = "shcore.dll\0".encode_utf16().collect();
        let shcore = unsafe {
            let handle = LoadLibraryExW(
                name.as_ptr(),
                std::ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_SYSTEM32,
            );
            if handle.is_null() {
                None
            } else {
                Some(Module(handle))
            }
        };
        Self {
            set_process_context: user32
                .as_ref()
                .and_then(|m| m.get_symbol("SetProcessDpiAwarenessContext").ok()),
            set_process_awareness: shcore
                .as_ref()
                .and_then(|m| m.get_symbol("SetProcessDpiAwareness").ok()),
            get_window_dpi: user32
                .as_ref()
                .and_then(|m| m.get_symbol("GetDpiForWindow").ok()),
            get_monitor_dpi: shcore
                .as_ref()
                .and_then(|m| m.get_symbol("GetDpiForMonitor").ok()),
            adjust_for_dpi: user32
                .as_ref()
                .and_then(|m| m.get_symbol("AdjustWindowRectExForDpi").ok()),
            _user32: user32,
            _shcore: shcore,
        }
    }

    /// Call before creating any HWND. Never replace a thread awareness context.
    pub(super) unsafe fn prepare_process(&self, high_dpi: bool) {
        if !high_dpi {
            return;
        }
        if let Some(set_context) = self.set_process_context {
            for context in [
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE,
            ] {
                if set_context(context) != 0 {
                    return;
                }
                // ACCESS_DENIED means the manifest/host already chose a mode.
                // Only an unsupported context warrants trying an older API.
                if GetLastError() != ERROR_INVALID_PARAMETER {
                    return;
                }
            }
        }
        if let Some(set_awareness) = self.set_process_awareness {
            // In particular, do not fall back after E_ACCESSDENIED.
            let _ = set_awareness(PROCESS_PER_MONITOR_DPI_AWARE);
            return;
        }
        // Vista/Windows 7 fallback: system awareness only. If this fails, the
        // query below observes the existing awareness rather than assuming success.
        let _ = SetProcessDPIAware();
    }

    pub(super) unsafe fn window_dpi(&self, hwnd: HWND, dc: HDC) -> u32 {
        if let Some(get_dpi) = self.get_window_dpi {
            // A failed HWND query must not be replaced with an unrelated monitor's
            // DPI (or a process-wide value that ignores a host's thread override).
            return match get_dpi(hwnd) {
                0 => BASE_DPI,
                dpi => dpi,
            };
        }
        // Pre-Windows 10 1607: there are no per-thread awareness contexts. X/Y
        // values from the effective-monitor query depend on process awareness.
        if let Some(get_dpi) = self.get_monitor_dpi {
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            if !monitor.is_null() {
                let (mut x, mut y) = (0, 0);
                if get_dpi(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) == 0 && x > 0 && y > 0 {
                    return x;
                }
            }
        }
        let dpi = if dc.is_null() {
            0
        } else {
            GetDeviceCaps(dc, LOGPIXELSX)
        };
        if dpi > 0 {
            dpi as u32
        } else {
            BASE_DPI
        }
    }

    pub(super) unsafe fn outer_size(
        &self,
        size: (i32, i32),
        style: DWORD,
        ex_style: DWORD,
        dpi: u32,
    ) -> Option<(i32, i32)> {
        let client = RECT {
            left: 0,
            top: 0,
            right: size.0,
            bottom: size.1,
        };
        let mut rect = client;
        let adjusted = self
            .adjust_for_dpi
            .is_some_and(|adjust| adjust(&mut rect, style, 0, ex_style, dpi) != 0);
        if !adjusted {
            // Do not reuse a partially modified RECT after an unsuccessful call.
            rect = client;
            if AdjustWindowRectEx(&mut rect, style, 0, ex_style) == 0 {
                return None;
            }
        }
        Some((
            rect.right.checked_sub(rect.left)?,
            rect.bottom.checked_sub(rect.top)?,
        ))
    }

    /// Call without a display/handler borrow or native-display lock: SetWindowPos
    /// can synchronously re-enter the window procedure.
    pub(super) unsafe fn resize_client(
        &self,
        hwnd: HWND,
        size: (i32, i32),
        style: DWORD,
        ex_style: DWORD,
        dpi: u32,
    ) -> bool {
        self.resize_client_at(hwnd, size, style, ex_style, dpi, None)
    }

    pub(super) unsafe fn resize_client_at(
        &self,
        hwnd: HWND,
        size: (i32, i32),
        style: DWORD,
        ex_style: DWORD,
        dpi: u32,
        position: Option<(i32, i32)>,
    ) -> bool {
        let (width, height) = match self.outer_size(size, style, ex_style, dpi) {
            Some(size) => size,
            None => return false,
        };
        let flags = SWP_NOZORDER | SWP_NOACTIVATE;
        let (x, y) = position.unwrap_or((0, 0));
        let move_flag = if position.is_some() { 0 } else { SWP_NOMOVE };
        if SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            x,
            y,
            width,
            height,
            flags | move_flag,
        ) == 0
        {
            return false;
        }

        // Legacy border metrics can use system DPI on a different-DPI monitor.
        // Reconcile once from measured client/outer rectangles, not in a resize
        // loop. OS constraints may still win; callers publish the actual size.
        let (mut client, mut outer): (RECT, RECT) = (std::mem::zeroed(), std::mem::zeroed());
        if GetClientRect(hwnd, &mut client) != 0 && GetWindowRect(hwnd, &mut outer) != 0 {
            let actual = (client.right - client.left, client.bottom - client.top);
            if actual != size {
                let width = i64::from(outer.right) - i64::from(outer.left) + i64::from(size.0)
                    - i64::from(actual.0);
                let height = i64::from(outer.bottom) - i64::from(outer.top) + i64::from(size.1)
                    - i64::from(actual.1);
                if let (Ok(width), Ok(height)) = (i32::try_from(width), i32::try_from(height)) {
                    if width > 0 && height > 0 {
                        let _ = SetWindowPos(
                            hwnd,
                            std::ptr::null_mut(),
                            0,
                            0,
                            width,
                            height,
                            flags | SWP_NOMOVE,
                        );
                    }
                }
            }
        }
        true
    }
}

/// A rational logical size: retain the original pixels and their rendering DPI
/// instead of deriving the next request from a previously rounded DPI resize.
#[derive(Clone, Copy)]
pub(super) struct ClientSize {
    pixels: (i32, i32),
    dpi: u32,
}

impl ClientSize {
    pub(super) fn new(pixels: (i32, i32), dpi: u32) -> Self {
        Self { pixels, dpi }
    }

    pub(super) fn at_dpi(self, dpi: u32) -> Option<(i32, i32)> {
        if self.dpi == 0 || dpi == 0 || self.pixels.0 <= 0 || self.pixels.1 <= 0 {
            return None;
        }
        let scale = |pixels: i32| {
            let value =
                (pixels as u64 * u64::from(dpi) + u64::from(self.dpi / 2)) / u64::from(self.dpi);
            i32::try_from(value.max(1)).ok()
        };
        Some((scale(self.pixels.0)?, scale(self.pixels.1)?))
    }
}

/// Round once at the final pixel boundary, with checked Win32 dimension limits.
pub(super) fn startup_pixels(logical: i32, dpi: u32) -> Option<i32> {
    if logical <= 0 || dpi == 0 {
        return None;
    }
    let pixels = (u64::try_from(logical).ok()? * u64::from(dpi) + u64::from(BASE_DPI / 2))
        / u64::from(BASE_DPI);
    i32::try_from(pixels.max(1)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use winapi::{shared::winerror::ERROR_ACCESS_DENIED, um::errhandlingapi::SetLastError};

    // Type-check the optional entry points without changing dynamic loading.
    const _: SetProcessContext = winapi::um::winuser::SetProcessDpiAwarenessContext;
    const _: SetProcessAwareness = winapi::um::shellscalingapi::SetProcessDpiAwareness;
    const _: GetWindowDpi = winapi::um::winuser::GetDpiForWindow;
    const _: GetMonitorDpi = winapi::um::shellscalingapi::GetDpiForMonitor;
    const _: AdjustForDpi = winapi::um::winuser::AdjustWindowRectExForDpi;

    #[test]
    fn transition_size_intent_does_not_accumulate_rounding() {
        let logical = ClientSize::new((801, 601), BASE_DPI);
        for _ in 0..100 {
            for dpi in [96, 120, 144, 168, 192, 168, 144, 120, 96] {
                assert_eq!(
                    logical.at_dpi(dpi),
                    Some((
                        startup_pixels(801, dpi).unwrap(),
                        startup_pixels(601, dpi).unwrap(),
                    ))
                );
            }
        }
        let runtime = ClientSize::new((1173, 853), 168);
        assert_eq!(runtime.at_dpi(96), Some((670, 487)));
        assert_eq!(runtime.at_dpi(168), Some((1173, 853)));
        assert_eq!(ClientSize::new((i32::MAX, 1), 96).at_dpi(192), None);
        assert_eq!(ClientSize::new((0, 1), 96).at_dpi(120), None);
        assert_eq!(ClientSize::new((1, 1), 0).at_dpi(120), None);
    }

    #[test]
    fn normal_and_fullscreen_creation_stay_hidden_until_wgl_is_ready() {
        struct Window(HWND, HDC);
        impl Drop for Window {
            fn drop(&mut self) {
                unsafe {
                    winapi::um::winuser::ReleaseDC(self.0, self.1);
                    winapi::um::winuser::DestroyWindow(self.0);
                }
            }
        }
        let api = DpiApi::load();
        for fullscreen in [false, true] {
            let conf = crate::conf::Conf {
                window_title: "Hidden DPI startup test".into(),
                fullscreen,
                high_dpi: false,
                ..Default::default()
            };
            unsafe {
                let (hwnd, dc, dpi) = super::super::create_window(&conf, &api);
                let window = Window(hwnd, dc);
                assert!(dpi > 0);
                assert_eq!(winapi::um::winuser::IsWindowVisible(window.0), 0);
            }
        }
    }

    thread_local! {
        static CALLS: RefCell<Vec<isize>> = RefCell::new(Vec::new());
        static CONTEXT_ERROR: Cell<u32> = Cell::new(0);
    }

    unsafe extern "system" fn set_context(context: DPI_AWARENESS_CONTEXT) -> BOOL {
        CALLS.with(|calls| calls.borrow_mut().push(context as isize));
        let error = CONTEXT_ERROR.with(Cell::get);
        if error == ERROR_ACCESS_DENIED || context == DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 {
            SetLastError(error);
            0
        } else {
            1
        }
    }

    unsafe extern "system" fn set_awareness(awareness: PROCESS_DPI_AWARENESS) -> HRESULT {
        CALLS.with(|calls| calls.borrow_mut().push(awareness as isize));
        0
    }

    #[test]
    fn awareness_fallback_does_not_override_a_host_or_low_dpi_preference() {
        let api = DpiApi {
            set_process_context: Some(set_context),
            set_process_awareness: Some(set_awareness),
            ..Default::default()
        };
        CALLS.with(|calls| calls.borrow_mut().clear());
        unsafe { api.prepare_process(false) };
        CALLS.with(|calls| assert!(calls.borrow().is_empty()));

        CONTEXT_ERROR.with(|error| error.set(ERROR_ACCESS_DENIED));
        unsafe { api.prepare_process(true) };
        CALLS.with(|calls| assert_eq!(*calls.borrow(), [-4]));

        CALLS.with(|calls| calls.borrow_mut().clear());
        CONTEXT_ERROR.with(|error| error.set(ERROR_INVALID_PARAMETER));
        unsafe { api.prepare_process(true) };
        CALLS.with(|calls| assert_eq!(*calls.borrow(), [-4, -3]));

        CALLS.with(|calls| calls.borrow_mut().clear());
        let api = DpiApi {
            set_process_context: None,
            ..api
        };
        unsafe { api.prepare_process(true) };
        CALLS.with(|calls| assert_eq!(*calls.borrow(), [2]));
    }

    unsafe extern "system" fn zero_window_dpi(_: HWND) -> UINT {
        0
    }

    unsafe extern "system" fn failed_monitor_dpi(
        _: HMONITOR,
        _: MONITOR_DPI_TYPE,
        x: *mut UINT,
        y: *mut UINT,
    ) -> HRESULT {
        // Failed queries may leave nonzero output values; they are not valid DPI.
        *x = 168;
        *y = 168;
        -1
    }

    #[test]
    fn missing_or_failed_queries_have_a_nonzero_base_dpi_fallback() {
        let null = std::ptr::null_mut();
        let mut api = DpiApi::default();
        unsafe {
            assert_eq!(api.window_dpi(null, std::ptr::null_mut()), 96);
            api.get_monitor_dpi = Some(failed_monitor_dpi);
            assert_eq!(api.window_dpi(null, std::ptr::null_mut()), 96);
            api.get_window_dpi = Some(zero_window_dpi);
            assert_eq!(api.window_dpi(null, std::ptr::null_mut()), 96);
        }
    }

    unsafe extern "system" fn failed_adjust(
        rect: *mut RECT,
        _: DWORD,
        _: BOOL,
        _: DWORD,
        _: UINT,
    ) -> BOOL {
        (*rect).right = 1;
        (*rect).bottom = 1;
        0
    }

    #[test]
    fn failed_dpi_border_query_restarts_legacy_calculation_from_client_size() {
        let api = DpiApi {
            adjust_for_dpi: Some(failed_adjust),
            ..Default::default()
        };
        let style = winapi::um::winuser::WS_OVERLAPPEDWINDOW;
        let mut expected = RECT {
            left: 0,
            top: 0,
            right: 801,
            bottom: 601,
        };
        unsafe {
            assert_ne!(AdjustWindowRectEx(&mut expected, style, 0, 0), 0);
            assert_eq!(
                api.outer_size((801, 601), style, 0, 168),
                Some((
                    expected.right - expected.left,
                    expected.bottom - expected.top
                ))
            );
        }
    }

    #[test]
    fn startup_sizes_round_only_at_the_pixel_boundary() {
        // Known results include half-pixel ties and fractional odd-size requests.
        for (dpi, regular, odd) in [
            (96, (800, 600), (801, 601)),
            (120, (1000, 750), (1001, 751)),
            (144, (1200, 900), (1202, 902)),
            (168, (1400, 1050), (1402, 1052)),
            (192, (1600, 1200), (1602, 1202)),
        ] {
            assert_eq!(
                (startup_pixels(800, dpi), startup_pixels(600, dpi)),
                (Some(regular.0), Some(regular.1))
            );
            assert_eq!(
                (startup_pixels(801, dpi), startup_pixels(601, dpi)),
                (Some(odd.0), Some(odd.1))
            );
        }
        assert_eq!(startup_pixels(i32::MAX, 96), Some(i32::MAX));
        assert_eq!(startup_pixels(i32::MAX, u32::MAX), None);
        assert_eq!(startup_pixels(800, 0), None);
        assert_eq!(startup_pixels(0, 168), None);
        assert_eq!(startup_pixels(-1, 168), None);
    }
}
