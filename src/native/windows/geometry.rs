//! Geometry transactions and callback delivery shared by the main and modal loops.
use super::*;

impl WindowsDisplay {
    fn render_dpi(&self) -> u32 {
        if self.high_dpi {
            self.window_dpi
        } else {
            dpi::BASE_DPI
        }
    }

    unsafe fn is_normal(this: *mut Self) -> bool {
        !(*this).fullscreen && IsIconic((*this).wnd) == 0 && IsZoomed((*this).wnd) == 0
    }

    // The DPI API is a disjoint, immutable field. No reference to the display or
    // its event handler survives a call that can synchronously dispatch messages.
    unsafe fn resize_normal(this: *mut Self, position: Option<(i32, i32)>) {
        let hwnd = (*this).wnd;
        let size = (*this).normal_size.at_dpi((*this).render_dpi());
        if let Some(size) = size {
            let style = GetWindowLongA(hwnd, GWL_STYLE) as DWORD;
            let ex_style = GetWindowLongA(hwnd, GWL_EXSTYLE) as DWORD;
            (*this).dpi_api.resize_client_at(
                hwnd,
                size,
                style,
                ex_style,
                (*this).window_dpi,
                position,
            );
            let mut client: RECT = std::mem::zeroed();
            if GetClientRect(hwnd, &mut client) != 0 {
                (*this).normal_client_size =
                    (client.right - client.left, client.bottom - client.top);
            }
        }
    }

    pub(super) unsafe fn change_dpi(this: *mut Self, dpi: u32, suggested: RECT) {
        (*this).pending_dpi = Some((dpi, suggested));
        (*this).update_requested = true;
        // GetMessage dispatches sent messages internally and can go back to sleep
        // without returning. A posted message wakes that path even with no input.
        PostMessageW((*this).wnd, WM_NULL, 0, 0);
    }

    unsafe fn apply_dpi(this: *mut Self, dpi: u32, suggested: RECT) {
        (*this).geometry_depth += 1;
        (*this).window_dpi = dpi;
        (*this).dpi_changed = true;
        (*this).update_requested = true;
        let hwnd = (*this).wnd;
        if Self::is_normal(this) {
            // Use only the suggested placement; size comes from stable logical
            // intent, rounded once, with target-DPI borders (never scaled twice).
            Self::resize_normal(this, Some((suggested.left, suggested.top)));
        } else {
            (*this).restore_size_pending = true;
            if IsIconic(hwnd) == 0 {
                let mut rect = suggested;
                if (*this).fullscreen {
                    // Stay borderless and cover the destination monitor. This
                    // does not change the existing primary-monitor entry policy.
                    let monitor = MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST);
                    let mut info: MONITORINFO = std::mem::zeroed();
                    info.cbSize = std::mem::size_of::<MONITORINFO>() as _;
                    if GetMonitorInfoW(monitor, &mut info) != 0 {
                        rect = info.rcMonitor;
                    }
                }
                if let (Some(width), Some(height)) = (
                    rect.right.checked_sub(rect.left),
                    rect.bottom.checked_sub(rect.top),
                ) {
                    if width > 0 && height > 0 {
                        SetWindowPos(
                            hwnd,
                            NULL as _,
                            rect.left,
                            rect.top,
                            width,
                            height,
                            SWP_NOZORDER | SWP_NOACTIVATE,
                        );
                    }
                }
            }
        }
        (*this).geometry_depth -= 1;
    }

    pub(super) unsafe fn size_changed(this: *mut Self) {
        if (*this).geometry_depth != 0 || (*this).pending_dpi.is_some() || !Self::is_normal(this) {
            return;
        }
        if (*this).restore_size_pending {
            (*this).restore_size_pending = false;
            (*this).geometry_depth += 1;
            Self::resize_normal(this, None);
            (*this).geometry_depth -= 1;
        } else {
            // A user/native resize replaces intent. Our own rounded or
            // OS-constrained DPI resize never feeds back into the next request.
            let mut rect: RECT = std::mem::zeroed();
            if GetClientRect((*this).wnd, &mut rect) != 0 {
                let size = (rect.right - rect.left, rect.bottom - rect.top);
                let previous = (*this).normal_client_size;
                if size.0 > 0 && size.1 > 0 && size != previous {
                    (*this).normal_client_size = size;
                    (*this).normal_size = dpi::ClientSize::new(size, (*this).render_dpi());
                }
            }
        }
    }

    /// Publish scale, actual client pixels and position in one lock acquisition.
    /// Only framebuffer changes require resize_event; moves alone do not.
    pub(super) unsafe fn publish_geometry(this: *mut Self) -> Option<(f32, f32)> {
        let hwnd = (*this).wnd;
        let (mut rect, mut client): (RECT, RECT) = (std::mem::zeroed(), std::mem::zeroed());
        if GetWindowRect(hwnd, &mut rect) == 0 || GetClientRect(hwnd, &mut client) == 0 {
            return None;
        }
        if (*this).cursor_grabbed && IsIconic(hwnd) == 0 {
            update_clip_rect(hwnd);
        }
        let size = (
            (client.right - client.left).max(1),
            (client.bottom - client.top).max(1),
        );
        let scale = (*this).render_dpi() as f32 / dpi::BASE_DPI as f32;
        (*this).window_scale = scale;
        let mut d = crate::native_display().lock().unwrap();
        let resized = size != (d.screen_width, d.screen_height);
        if resized || scale != d.dpi_scale {
            (*this).update_requested = true;
        }
        d.dpi_scale = scale;
        d.screen_width = size.0;
        d.screen_height = size.1;
        // Keep signed native coordinates until the existing public API boundary.
        d.screen_position = (rect.left as u32, rect.top as u32);
        drop(d);
        if std::mem::replace(&mut (*this).dpi_changed, false) && !(*this).cursor_grabbed {
            // A stationary pointer may have different client coordinates after
            // the transition. Refresh downstream caches without touching raw
            // relative hardware units or moving the OS cursor.
            let mut point: POINT = std::mem::zeroed();
            if GetCursorPos(&mut point) != 0 && ScreenToClient(hwnd, &mut point) != 0 {
                (*this).mouse_x = point.x as f32;
                (*this).mouse_y = point.y as f32;
                (*this)
                    .events
                    .mouse_motion_event(point.x as f32, point.y as f32);
            }
        }
        if resized {
            Some((size.0 as f32, size.1 as f32))
        } else {
            None
        }
    }

    pub(super) unsafe fn flush_events(this: *mut Self) {
        if (*this).geometry_depth != 0 {
            return;
        }
        // Owning the handler locally prevents aliasing if user code itself calls
        // Win32. Nested messages are queued and delivered after the callback.
        let mut handler = match (*this).event_handler.take() {
            Some(handler) => handler,
            None => return,
        };
        loop {
            // A resize can select another monitor and send another DPI message.
            // Finish the latest transition before publishing or calling user code.
            for _ in 0..4 {
                match (*this).pending_dpi.take() {
                    Some((dpi, rect)) => Self::apply_dpi(this, dpi, rect),
                    None => break,
                }
            }
            if (*this).pending_dpi.is_some() {
                // Bound synchronous work if Windows keeps constraining geometry.
                // The main/modal loop will resume the remaining transition.
                PostMessageW((*this).wnd, WM_NULL, 0, 0);
                break;
            }
            if let Some((width, height)) = Self::publish_geometry(this) {
                handler.resize_event(width, height);
                // A callback may itself have caused another native transition.
                continue;
            }
            let event = match (*this).events.0.pop_front() {
                Some(event) => event,
                None => break,
            };
            if matches!(event, events::Event::Quit) {
                let ordered = {
                    let mut d = crate::native_display().lock().unwrap();
                    if !d.quit_ordered {
                        d.quit_requested = true;
                    }
                    d.quit_ordered
                };
                if !ordered {
                    handler.quit_requested_event();
                }
                let mut d = crate::native_display().lock().unwrap();
                if d.quit_requested {
                    d.quit_ordered = true;
                }
                let ordered = d.quit_ordered;
                drop(d);
                if ordered {
                    PostQuitMessage(0);
                }
            } else {
                event.dispatch(handler.as_mut());
            }
        }
        (*this).event_handler = Some(handler);
    }

    pub(super) unsafe fn frame(this: *mut Self, force: bool) {
        Self::flush_events(this);
        if (*this).geometry_depth != 0
            || (*this).pending_dpi.is_some()
            || (!force && !(*this).update_requested)
        {
            return;
        }
        if let Some(mut handler) = (*this).event_handler.take() {
            (*this).update_requested = false;
            handler.update();
            (*this).event_handler = Some(handler);
            Self::flush_events(this);
            if (*this).pending_dpi.is_some() {
                return;
            }
            let mut handler = (*this).event_handler.take().unwrap();
            handler.draw();
            (*this).event_handler = Some(handler);
            SwapBuffers((*this).dc);
        }
    }

    unsafe fn set_fullscreen(this: *mut Self, fullscreen: bool) {
        if (*this).fullscreen == fullscreen {
            return;
        }
        let hwnd = (*this).wnd;
        if fullscreen {
            let mut placement: WINDOWPLACEMENT = std::mem::zeroed();
            placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as _;
            if GetWindowPlacement(hwnd, &mut placement) != 0 {
                (*this).windowed_placement = Some(placement);
            }
        }
        (*this).fullscreen = fullscreen;
        let style = get_win_style(fullscreen, (*this).window_resizable);
        SetWindowLongW(hwnd, GWL_STYLE, style as _);
        if fullscreen {
            SetWindowPos(
                hwnd,
                HWND_TOP,
                0,
                0,
                GetSystemMetrics(SM_CXSCREEN),
                GetSystemMetrics(SM_CYSCREEN),
                SWP_FRAMECHANGED,
            );
        } else {
            if let Some(placement) = (*this).windowed_placement.take() {
                // WINDOWPLACEMENT uses workspace coordinates: restore it through
                // SetWindowPlacement, never pass its position to SetWindowPos.
                SetWindowPlacement(hwnd, &placement);
            }
            SetWindowPos(
                hwnd,
                NULL as _,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
            if Self::is_normal(this) {
                (*this).restore_size_pending = false;
                Self::resize_normal(this, None);
            }
        }
        ShowWindow(hwnd, SW_SHOW);
    }

    pub(super) unsafe fn process_request(this: *mut Self, request: Request) {
        use Request::*;
        (*this).geometry_depth += 1;
        match request {
            ScheduleUpdate => (*this).update_requested = true,
            SetWindowSize {
                new_width,
                new_height,
            } => {
                if new_width > 0
                    && new_height > 0
                    && new_width <= i32::MAX as u32
                    && new_height <= i32::MAX as u32
                {
                    (*this).normal_size = dpi::ClientSize::new(
                        (new_width as i32, new_height as i32),
                        (*this).render_dpi(),
                    );
                    if Self::is_normal(this) {
                        Self::resize_normal(this, None);
                    } else {
                        (*this).restore_size_pending = true;
                    }
                }
            }
            SetWindowPosition { new_x, new_y } => {
                SetWindowPos(
                    (*this).wnd,
                    NULL as _,
                    new_x as i32,
                    new_y as i32,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            SetFullscreen(fullscreen) => Self::set_fullscreen(this, fullscreen),
            SetCursorGrab(grab) => Self::set_cursor_grab(this, grab),
            ShowMouse(show) => Self::show_mouse(this, show),
            SetMouseCursor(icon) => Self::set_mouse_cursor(this, icon),
            ShowKeyboard(show) => {
                IME_USER_DISABLED.store(!show, std::sync::atomic::Ordering::Relaxed)
            }
            SetImePosition { x, y } => Self::set_ime_position(this, x, y),
            SetImeEnabled(enabled) => Self::set_ime_enabled(this, enabled),
        }
        (*this).geometry_depth -= 1;
        Self::flush_events(this);
    }
}
