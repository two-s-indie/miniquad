#![cfg(target_os = "windows")]

//! Real HWND/WGL/message-loop tests with injected WM_DPICHANGED messages. They
//! exercise engine transitions, not Windows' monitor selection or DPI delivery.
use miniquad::{conf::Conf, window, EventHandler};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

static ORIGINAL_WNDPROC: AtomicIsize = AtomicIsize::new(0);
static MODAL_TIMER: AtomicUsize = AtomicUsize::new(0);
static IN_TIMER: AtomicBool = AtomicBool::new(false);
static EMPTY_CLIENT: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn probe_wndproc(hwnd: HWND, msg: u32, wp: usize, lp: isize) -> isize {
    let timer = msg == WM_TIMER;
    if timer {
        MODAL_TIMER.store(wp, Ordering::SeqCst);
        IN_TIMER.store(true, Ordering::SeqCst);
    }
    let result = CallWindowProcW(
        std::mem::transmute(ORIGINAL_WNDPROC.load(Ordering::SeqCst)),
        hwnd,
        msg,
        wp,
        lp,
    );
    if msg == WM_NCCALCSIZE && EMPTY_CLIENT.load(Ordering::SeqCst) {
        // Produce an actual empty normal client area through the documented
        // non-client calculation hook, rather than mocking our own geometry code.
        let rect = if wp != 0 {
            &mut (*(lp as *mut NCCALCSIZE_PARAMS)).rgrc[0]
        } else {
            &mut *(lp as *mut RECT)
        };
        rect.right = rect.left;
        rect.bottom = rect.top;
    }
    if timer {
        IN_TIMER.store(false, Ordering::SeqCst);
    }
    result
}
use winapi::{
    shared::{minwindef::BOOL, windef::*},
    um::{
        libloaderapi::{GetModuleHandleA, GetProcAddress},
        winuser::*,
    },
};

#[derive(Clone, Copy, Debug, Default)]
struct Observed {
    frames: usize,
    resizes: usize,
    motions: usize,
    scale: f32,
    size: (f32, f32),
    mouse: (f32, f32),
    resize_scale: f32,
    reenter_dpi: Option<u32>,
    timer_frames: usize,
    quits: usize,
    quit_finished: bool,
    keys: usize,
}

struct App {
    hwnd: HWND,
    observed: Arc<Mutex<Observed>>,
}
impl App {
    fn check(&self) {
        assert!(
            !self.observed.lock().unwrap().quit_finished,
            "callback after accepted quit"
        );
        unsafe {
            let mut client: RECT = std::mem::zeroed();
            assert_ne!(GetClientRect(self.hwnd, &mut client), 0);
            assert_eq!(
                window::screen_size(),
                (
                    (client.right - client.left).max(1) as f32,
                    (client.bottom - client.top).max(1) as f32,
                ),
                "callback observed stale framebuffer dimensions"
            );
            let rect = outer(self.hwnd);
            assert_eq!(
                window::get_window_position(),
                (rect.left as u32, rect.top as u32),
                "callback observed a different position snapshot"
            );
        }
    }
}
impl EventHandler for App {
    fn update(&mut self) {
        self.check();
    }
    fn draw(&mut self) {
        self.check();
        let mut o = self.observed.lock().unwrap();
        o.frames += 1;
        if IN_TIMER.load(Ordering::SeqCst) {
            o.timer_frames += 1;
        }
        o.scale = window::dpi_scale();
        o.size = window::screen_size();
    }
    fn resize_event(&mut self, width: f32, height: f32) {
        self.check();
        assert_eq!((width, height), window::screen_size());
        let mut o = self.observed.lock().unwrap();
        o.resizes += 1;
        o.scale = window::dpi_scale();
        o.resize_scale = window::dpi_scale();
        o.size = (width, height);
        let reenter = o.reenter_dpi.take();
        drop(o);
        if let Some(dpi) = reenter {
            unsafe {
                change(self.hwnd, dpi, outer(self.hwnd));
            }
            // Nested window messages must queue work rather than borrowing this
            // event handler again or publishing halfway through this callback.
            assert_eq!((width, height), window::screen_size());
        }
    }
    fn mouse_motion_event(&mut self, x: f32, y: f32) {
        self.check();
        let mut o = self.observed.lock().unwrap();
        o.motions += 1;
        o.mouse = (x, y);
    }
    fn window_minimized_event(&mut self) {
        self.check();
    }
    fn window_restored_event(&mut self) {
        self.check();
    }
    fn quit_requested_event(&mut self) {
        self.check();
        let mut o = self.observed.lock().unwrap();
        o.quits += 1;
        if o.quits == 1 {
            window::cancel_quit();
        } else {
            o.quit_finished = true;
        }
    }
    fn key_down_event(&mut self, _: miniquad::KeyCode, _: miniquad::KeyMods, _: bool) {
        self.check();
        self.observed.lock().unwrap().keys += 1;
    }
}

fn wait_for(
    observed: &Mutex<Observed>,
    label: &str,
    predicate: impl Fn(Observed) -> bool,
) -> Observed {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let current = *observed.lock().unwrap();
        if predicate(current) {
            return current;
        }
        assert!(
            Instant::now() < deadline,
            "{}: timed out, last={:?}",
            label,
            current
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

unsafe fn outer(hwnd: HWND) -> RECT {
    let mut rect = std::mem::zeroed();
    assert_ne!(GetWindowRect(hwnd, &mut rect), 0);
    rect
}

unsafe fn change(hwnd: HWND, dpi: u32, rect: RECT) {
    // SendMessage keeps this stack rectangle alive until the receiver copies it.
    SendMessageW(
        hwnd,
        WM_DPICHANGED,
        (dpi | dpi << 16) as _,
        &rect as *const _ as _,
    );
}

fn pixels(size: (u32, u32), dpi: u32, base: u32) -> (f32, f32) {
    (
        ((size.0 * dpi + base / 2) / base) as f32,
        ((size.1 * dpi + base / 2) / base) as f32,
    )
}

unsafe fn exercise(hwnd: HWND, observed: Arc<Mutex<Observed>>, low: bool) {
    wait_for(&observed, "initial frame", |o| o.frames > 0);
    // A move alone updates position without causing resize callbacks. A duplicate
    // WM_SIZE contains only 16-bit dimensions, so it must never replace the HWND
    // query with its deliberately misleading payload.
    let before = *observed.lock().unwrap();
    SetWindowPos(
        hwnd,
        std::ptr::null_mut(),
        -41,
        -23,
        0,
        0,
        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
    );
    SendMessageW(hwnd, WM_SIZE, SIZE_RESTORED, 7 | (9 << 16));
    assert_eq!(
        window::get_window_position(),
        (-41i32 as u32, -23i32 as u32)
    );
    assert_eq!(observed.lock().unwrap().resizes, before.resizes);
    assert_eq!(window::screen_size(), before.size);
    let rect = outer(hwnd);
    EMPTY_CLIENT.store(true, Ordering::SeqCst);
    SetWindowPos(
        hwnd,
        std::ptr::null_mut(),
        0,
        0,
        rect.right - rect.left + 1,
        rect.bottom - rect.top + 1,
        SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
    );
    let mut client: RECT = std::mem::zeroed();
    assert_ne!(GetClientRect(hwnd, &mut client), 0);
    assert_eq!((client.right, client.bottom), (0, 0));
    let suspended = *observed.lock().unwrap();
    SendMessageW(hwnd, WM_KEYDOWN, VK_F24 as _, 0);
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(
        observed.lock().unwrap().frames,
        suspended.frames,
        "drew with invalid normal geometry"
    );
    assert_eq!(
        observed.lock().unwrap().keys,
        suspended.keys,
        "delivered input with invalid geometry"
    );
    assert_eq!(
        window::screen_size(),
        before.size,
        "published a fabricated empty-client resize"
    );
    EMPTY_CLIENT.store(false, Ordering::SeqCst);
    SetWindowPos(
        hwnd,
        std::ptr::null_mut(),
        0,
        0,
        rect.right - rect.left,
        rect.bottom - rect.top,
        SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
    );
    let recovered = wait_for(&observed, "geometry recovery and queued input", |o| {
        o.frames > suspended.frames && o.keys == suspended.keys + 1
    });
    assert_eq!(
        recovered.resizes, before.resizes,
        "invalid intermediate rectangle generated resize callbacks"
    );
    // Start each normal transition at the same signed desktop position, safely
    // overlapping the primary monitor even when its top-left corner is negative.
    let mut suggested = outer(hwnd);
    suggested.left = -20;
    suggested.top = -10;
    let scale = |dpi| if low { 1.0 } else { dpi as f32 / 96.0 };
    let expected = |size, dpi, base| {
        if low {
            pixels(size, 96, 96)
        } else {
            pixels(size, dpi, base)
        }
    };
    for _ in 0..3 {
        for dpi in [96, 120, 144, 168, 192, 168, 144, 120, 96] {
            let before = *observed.lock().unwrap();
            let mut old_pointer: POINT = std::mem::zeroed();
            GetCursorPos(&mut old_pointer);
            change(hwnd, dpi, suggested);
            let after = wait_for(&observed, "DPI redraw", |o| {
                o.frames > before.frames
                    && o.scale == scale(dpi)
                    && o.size == expected((801, 601), dpi, 96)
            });
            assert!(
                after.resizes - before.resizes <= 1,
                "intermediate/duplicate resize callbacks: {:?} -> {:?}",
                before,
                after
            );
            assert!(
                after.motions > before.motions,
                "stationary pointer cache was not refreshed"
            );
            if after.resizes > before.resizes {
                assert_eq!(
                    after.resize_scale,
                    scale(dpi),
                    "resize callback saw the old scale"
                );
            }
            let rect = outer(hwnd);
            assert_eq!((rect.left, rect.top), (-20, -10));
            assert_eq!(
                window::get_window_position(),
                (-20i32 as u32, -10i32 as u32)
            );
            let mut pointer: POINT = std::mem::zeroed();
            GetCursorPos(&mut pointer);
            if (pointer.x, pointer.y) == (old_pointer.x, old_pointer.y) {
                ScreenToClient(hwnd, &mut pointer);
                assert_eq!(after.mouse, (pointer.x as f32, pointer.y as f32));
            }
        }
    }
    println!(
        "fractional round trips: low={low}, {:?}",
        *observed.lock().unwrap()
    );

    if !low {
        observed.lock().unwrap().reenter_dpi = Some(168);
        change(hwnd, 120, suggested);
        wait_for(&observed, "DPI message from resize callback", |o| {
            o.reenter_dpi.is_none() && o.scale == 1.75 && o.size == pixels((801, 601), 168, 96)
        });
    }

    // Application framebuffer requests replace intent without another DPI factor.
    change(hwnd, 168, suggested);
    wait_for(&observed, "175% before request", |o| o.scale == scale(168));
    window::set_window_size(1173, 853);
    PostMessageW(hwnd, WM_NULL, 0, 0); // wake the unrelated request channel
    wait_for(&observed, "runtime framebuffer request", |o| {
        o.size == (1173.0, 853.0)
    });
    change(hwnd, 120, suggested);
    wait_for(&observed, "runtime intent at 125%", |o| {
        o.size == expected((1173, 853), 120, 168)
    });
    change(hwnd, 168, suggested);
    wait_for(&observed, "runtime intent round trip", |o| {
        o.size == (1173.0, 853.0)
    });

    // Native/user resize updates intent as well. Compute the current border from
    // measured geometry, since injected DPI doesn't change Windows' actual DPI.
    let mut client: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut client);
    let rect = outer(hwnd);
    let before = *observed.lock().unwrap();
    SetWindowPos(
        hwnd,
        std::ptr::null_mut(),
        0,
        0,
        987 + rect.right - rect.left - client.right,
        731 + rect.bottom - rect.top - client.bottom,
        SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
    );
    // No posted wake-up and no mouse/input: the resize itself must wake GetMessage.
    let after = wait_for(&observed, "native resize and redraw", |o| {
        o.size == (987.0, 731.0) && o.frames > before.frames
    });
    assert_eq!(
        after.resizes,
        before.resizes + 1,
        "native resize was reported more than once"
    );
    change(hwnd, 120, suggested);
    wait_for(&observed, "native intent at 125%", |o| {
        o.size == expected((987, 731), 120, 168)
    });
    change(hwnd, 168, suggested);
    wait_for(&observed, "native intent round trip", |o| {
        o.size == (987.0, 731.0)
    });

    // Maximize produces a scale-only transition with identical framebuffer size.
    SendMessageW(hwnd, WM_SYSCOMMAND, SC_MAXIMIZE, 0);
    assert_ne!(IsZoomed(hwnd), 0);
    let before = *observed.lock().unwrap();
    let rect = outer(hwnd);
    change(hwnd, 192, rect);
    let after = wait_for(&observed, "maximized scale-only redraw", |o| {
        o.frames > before.frames && o.scale == scale(192)
    });
    assert_ne!(IsZoomed(hwnd), 0);
    assert_eq!(after.size, before.size);
    assert_eq!(after.resizes, before.resizes);
    SendMessageW(hwnd, WM_SYSCOMMAND, SC_RESTORE, 0);
    PostMessageW(hwnd, WM_NULL, 0, 0);
    wait_for(&observed, "maximized restore intent", |o| {
        o.size == expected((987, 731), 192, 168)
    });

    SendMessageW(hwnd, WM_SYSCOMMAND, SC_MINIMIZE, 0);
    assert_ne!(IsIconic(hwnd), 0);
    let before = *observed.lock().unwrap();
    change(hwnd, 120, suggested);
    wait_for(&observed, "minimized redraw", |o| {
        o.frames > before.frames && o.scale == scale(120)
    });
    assert_ne!(IsIconic(hwnd), 0);
    SendMessageW(hwnd, WM_SYSCOMMAND, SC_RESTORE, 0);
    PostMessageW(hwnd, WM_NULL, 0, 0);
    wait_for(&observed, "minimized restore intent", |o| {
        o.size == expected((987, 731), 120, 168)
    });

    // The modal timer must use the same publish/notify path as the normal loop.
    SendMessageW(hwnd, WM_ENTERSIZEMOVE, 0, 0);
    SendMessageW(hwnd, WM_ENTERMENULOOP, 0, 0);
    let before = *observed.lock().unwrap();
    change(hwnd, 168, suggested);
    let after = wait_for(&observed, "modal DPI redraw", |o| {
        o.frames > before.frames && o.size == (987.0, 731.0)
    });
    assert!(after.resizes - before.resizes <= 1);
    wait_for(&observed, "modal timer frame", |o| o.timer_frames > 0);
    let timer = MODAL_TIMER.load(Ordering::SeqCst);
    assert_ne!(timer, 0);
    SendMessageW(hwnd, WM_EXITMENULOOP, 0, 0);
    let before = *observed.lock().unwrap();
    wait_for(&observed, "outer modal loop still draws", |o| {
        o.timer_frames > before.timer_frames
    });
    SendMessageW(hwnd, WM_EXITSIZEMOVE, 0, 0);
    let before = *observed.lock().unwrap();
    // KillTimer doesn't remove already-queued messages; replay one explicitly.
    SendMessageW(hwnd, WM_TIMER, timer, 0);
    SendMessageW(hwnd, WM_TIMER, 0, 0);
    assert_eq!(
        observed.lock().unwrap().timer_frames,
        before.timer_frames,
        "stale modal timer drew after exiting the loop"
    );

    window::set_fullscreen(true);
    PostMessageW(hwnd, WM_NULL, 0, 0);
    wait_for(&observed, "fullscreen", |o| {
        o.size
            == (
                GetSystemMetrics(SM_CXSCREEN) as f32,
                GetSystemMetrics(SM_CYSCREEN) as f32,
            )
    });
    let before = *observed.lock().unwrap();
    change(hwnd, 192, outer(hwnd));
    let after = wait_for(&observed, "fullscreen DPI redraw", |o| {
        o.frames > before.frames && o.scale == scale(192)
    });
    assert_eq!(GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_CAPTION, 0);
    assert_eq!(after.size, before.size);
    window::set_fullscreen(false);
    PostMessageW(hwnd, WM_NULL, 0, 0);
    wait_for(&observed, "fullscreen restore intent", |o| {
        o.size == expected((987, 731), 192, 168)
    });
    assert_ne!(GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_CAPTION, 0);
    println!(
        "resize, scale-only, minimize/maximize, modal, fullscreen: {:?}",
        *observed.lock().unwrap()
    );
    SendMessageW(hwnd, WM_CLOSE, 0, 0);
    assert_eq!(observed.lock().unwrap().quits, 1);
    assert!(!observed.lock().unwrap().quit_finished);
    SendMessageW(hwnd, WM_CLOSE, 0, 0);
    assert!(observed.lock().unwrap().quit_finished);
}

#[test]
fn dpi_transitions() {
    let mode = match std::env::var("MINIQUAD_DPI_TRANSITION_CASE") {
        Ok(mode) => mode,
        Err(_) => {
            for case in ["high", "blocking", "low", "low-blocking"] {
                let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "dpi_transitions",
                        "--nocapture",
                        "--test-threads=1",
                    ])
                    .env("MINIQUAD_DPI_TRANSITION_CASE", case)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap();
                let deadline = Instant::now() + Duration::from_secs(45);
                while child.try_wait().unwrap().is_none() {
                    if Instant::now() >= deadline {
                        child.kill().unwrap();
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                let output = child.wait_with_output().unwrap();
                assert!(
                    output.status.success(),
                    "{case}:\n{}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                print!("{case}: {}", String::from_utf8_lossy(&output.stdout));
            }
            return;
        }
    };
    unsafe {
        let module = GetModuleHandleA(b"user32.dll\0".as_ptr().cast());
        let address = GetProcAddress(module, b"SetProcessDpiAwarenessContext\0".as_ptr().cast());
        if address.is_null() {
            println!("SKIP: per-monitor awareness API unavailable");
            return;
        }
        let set_context: unsafe extern "system" fn(DPI_AWARENESS_CONTEXT) -> BOOL =
            std::mem::transmute(address);
        assert_ne!(set_context(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2), 0);
    }
    let low = mode.starts_with("low");
    let mut conf = Conf {
        window_title: format!("DPI transition test {}", std::process::id()),
        window_width: 801,
        window_height: 601,
        high_dpi: !low,
        ..Default::default()
    };
    conf.platform.blocking_event_loop = mode.contains("blocking");
    let title: Vec<u16> = conf.window_title.encode_utf16().chain(Some(0)).collect();
    let observed = Arc::new(Mutex::new(Observed::default()));
    let worker = Arc::new(Mutex::new(None));
    let driver = worker.clone();
    miniquad::start(conf, move || unsafe {
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        assert!(!hwnd.is_null());
        #[cfg(target_pointer_width = "64")]
        let original = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, probe_wndproc as *const () as isize);
        #[cfg(target_pointer_width = "32")]
        let original =
            SetWindowLongW(hwnd, GWLP_WNDPROC, probe_wndproc as *const () as i32) as isize;
        assert_ne!(original, 0);
        ORIGINAL_WNDPROC.store(original, Ordering::SeqCst);
        // Native messages can arrive while the application factory is running,
        // before it returns an event handler. Keep public geometry current then.
        change(hwnd, 120, outer(hwnd));
        change(hwnd, 144, outer(hwnd));
        assert_eq!(window::dpi_scale(), if low { 1.0 } else { 1.5 });
        assert_eq!(
            window::screen_size(),
            if low { (801.0, 601.0) } else { (1202.0, 902.0) }
        );
        let address = hwnd as usize;
        let observations = observed.clone();
        *driver.lock().unwrap() = Some(std::thread::spawn(move || {
            let result = std::panic::catch_unwind(|| exercise(address as HWND, observations, low));
            PostMessageW(address as HWND, WM_CLOSE, 0, 0);
            if let Err(error) = result {
                std::panic::resume_unwind(error);
            }
        }));
        Box::new(App { hwnd, observed })
    });
    worker.lock().unwrap().take().unwrap().join().unwrap();
}
