//! Geometry transactions and callback delivery shared by the main and modal loops.
use super::*;

/// Native positions stay signed; conversion to the historical u32 public API
/// happens only when publishing a complete, validated snapshot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Geometry {
    pub(super) size: (i32, i32),
    position: (i32, i32),
    scale: f32,
}

impl Geometry {
    fn from_rects(client: RECT, outer: RECT, minimized: bool, scale: f32) -> Option<Self> {
        let width = client.right.checked_sub(client.left)?;
        let height = client.bottom.checked_sub(client.top)?;
        if width < 0
            || height < 0
            || (!minimized && (width == 0 || height == 0))
            || !scale.is_finite()
            || scale <= 0.0
        {
            return None;
        }
        Some(Self {
            // Preserve the nonzero minimized framebuffer policy. Empty normal
            // rectangles are transient/invalid, not a real 1x1 resize.
            size: (width.max(1), height.max(1)),
            position: (outer.left, outer.top),
            scale,
        })
    }

    unsafe fn query(hwnd: HWND, scale: f32) -> Option<Self> {
        let (mut outer, mut client): (RECT, RECT) = (std::mem::zeroed(), std::mem::zeroed());
        if GetWindowRect(hwnd, &mut outer) == 0 || GetClientRect(hwnd, &mut client) == 0 {
            return None;
        }
        Self::from_rects(client, outer, IsIconic(hwnd) != 0, scale)
    }
}

impl WindowsDisplay {
    pub(super) unsafe fn stop_modal_timer(this: *mut Self) {
        let timer = std::mem::replace(&mut (*this).modal_resizing_timer, 0);
        if timer != 0 {
            KillTimer((*this).wnd, timer);
        }
    }

    pub(super) unsafe fn detach(this: *mut Self) {
        Self::stop_modal_timer(this);
        (*this).modal_loop_depth = 0;
        #[cfg(target_pointer_width = "64")]
        SetWindowLongPtrW((*this).wnd, GWLP_USERDATA, 0);
        #[cfg(target_pointer_width = "32")]
        SetWindowLongW((*this).wnd, GWLP_USERDATA, 0);
    }

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
    /// None means query failure/invalid geometry, not "unchanged". Callers must
    /// defer callbacks and drawing until a complete valid snapshot is available.
    pub(super) unsafe fn publish_geometry(this: *mut Self) -> Option<Geometry> {
        let hwnd = (*this).wnd;
        let scale = (*this).render_dpi() as f32 / dpi::BASE_DPI as f32;
        let geometry = Geometry::query(hwnd, scale)?;
        if (*this).cursor_grabbed && IsIconic(hwnd) == 0 {
            update_clip_rect(hwnd);
        }
        (*this).window_scale = scale;
        (*this).geometry = Some(geometry);
        let mut d = crate::native_display().lock().unwrap();
        let resumed = std::mem::replace(&mut (*this).geometry_suspended, false);
        let changed =
            geometry.size != (d.screen_width, d.screen_height) || scale != d.dpi_scale || resumed;
        if changed {
            (*this).update_requested = true;
        }
        d.dpi_scale = scale;
        d.screen_width = geometry.size.0;
        d.screen_height = geometry.size.1;
        // Keep signed native coordinates until the existing public API boundary.
        d.screen_position = (geometry.position.0 as u32, geometry.position.1 as u32);
        drop(d);
        if changed {
            // Sent WM_SIZE messages can be dispatched inside GetMessage without
            // returning to the loop. Wake ordinary native resizes too, not only DPI.
            PostMessageW(hwnd, WM_NULL, 0, 0);
        }
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
        Some(geometry)
    }

    unsafe fn settle_geometry(this: *mut Self) -> Option<Geometry> {
        // A resize can select another monitor and send another DPI message.
        // Finish the latest transition before publishing or calling user code.
        for _ in 0..4 {
            match (*this).pending_dpi.take() {
                Some((dpi, rect)) => Self::apply_dpi(this, dpi, rect),
                None => break,
            }
        }
        if (*this).pending_dpi.is_some() {
            PostMessageW((*this).wnd, WM_NULL, 0, 0);
            return None;
        }
        let geometry = Self::publish_geometry(this);
        if geometry.is_none() {
            // Keep the previous snapshot and queued events. A later native
            // message can retry; a blocking loop need not spin on a failed query.
            (*this).update_requested = false;
            (*this).geometry_suspended = true;
        }
        geometry
    }

    pub(super) unsafe fn flush_events(this: *mut Self) -> bool {
        if (*this).geometry_depth != 0
            || (*this).notification_depth != 0
            || (*this).window_destroyed
            || crate::native_display().lock().unwrap().quit_ordered
        {
            return false;
        }
        // Owning the handler locally prevents aliasing if user code itself calls
        // Win32. Nested messages are queued and delivered after the callback.
        let mut handler = match (*this).event_handler.take() {
            Some(handler) => handler,
            None => {
                // Before the factory returns, maintain geometry without invoking
                // application callbacks. During a callback, defer all nested work.
                return (*this).initializing && Self::settle_geometry(this).is_some();
            }
        };
        let ready = loop {
            if (*this).window_destroyed || crate::native_display().lock().unwrap().quit_ordered {
                break false;
            }
            let geometry = match Self::settle_geometry(this) {
                Some(geometry) => geometry,
                None => break false,
            };
            if (*this).notified_size != Some(geometry.size) {
                (*this).notified_size = Some(geometry.size);
                handler.resize_event(geometry.size.0 as f32, geometry.size.1 as f32);
                // A callback may itself have caused another native transition.
                continue;
            }
            let event = match (*this).events.0.pop_front() {
                Some(event) => event,
                None => break true,
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
        };
        (*this).event_handler = Some(handler);
        ready
    }

    pub(super) unsafe fn frame(this: *mut Self, force: bool) {
        if !Self::flush_events(this) || (!force && !(*this).update_requested) {
            return;
        }
        if let Some(mut handler) = (*this).event_handler.take() {
            (*this).update_requested = false;
            handler.update();
            (*this).event_handler = Some(handler);
            if !Self::flush_events(this) {
                return;
            }
            let mut handler = (*this).event_handler.take().unwrap();
            handler.draw();
            (*this).event_handler = Some(handler);
            if !(*this).window_destroyed && !crate::native_display().lock().unwrap().quit_ordered {
                SwapBuffers((*this).dc);
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_geometry_keeps_fractional_pixels_and_signed_positions() {
        let client = RECT {
            left: 0,
            top: 0,
            right: 1402,
            bottom: 1052,
        };
        let outer = RECT {
            left: -1901,
            top: -701,
            right: -471,
            bottom: 418,
        };
        assert_eq!(
            Geometry::from_rects(client, outer, false, 1.75),
            Some(Geometry {
                size: (1402, 1052),
                position: (-1901, -701),
                scale: 1.75,
            })
        );
    }

    #[test]
    fn failed_and_invalid_geometry_never_become_resize_notifications() {
        let outer: RECT = unsafe { std::mem::zeroed() };
        let empty = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        assert!(unsafe { Geometry::query(std::ptr::null_mut(), 1.75) }.is_none());
        assert_eq!(Geometry::from_rects(empty, outer, false, 1.75), None);
        assert_eq!(
            Geometry::from_rects(empty, outer, true, 1.75).unwrap().size,
            (1, 1)
        );
        for client in [
            RECT {
                left: 0,
                top: 0,
                right: -1,
                bottom: 10,
            },
            RECT {
                left: i32::MIN,
                top: 0,
                right: i32::MAX,
                bottom: 10,
            },
        ] {
            assert_eq!(Geometry::from_rects(client, outer, false, 1.0), None);
            assert_eq!(Geometry::from_rects(client, outer, true, 1.0), None);
        }
        let valid = RECT {
            left: 0,
            top: 0,
            right: 10,
            bottom: 20,
        };
        for scale in [0.0, -1.0, f32::INFINITY, f32::NAN] {
            assert_eq!(Geometry::from_rects(valid, outer, false, scale), None);
        }
    }
}
