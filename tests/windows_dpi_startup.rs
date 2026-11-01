#![cfg(target_os = "windows")]

//! Each awareness mode needs a fresh process: Windows only sets its default once.
//! These tests use real HWND/WGL startup and inspect the first app callback.
use miniquad::{conf::Conf, window, EventHandler};
use winapi::{
    shared::{
        minwindef::{BOOL, UINT},
        windef::*,
    },
    um::{
        libloaderapi::{GetModuleHandleA, GetProcAddress},
        winuser::*,
    },
};

unsafe fn symbol<T: Copy>(name: &[u8]) -> Option<T> {
    let module = GetModuleHandleA(b"user32.dll\0".as_ptr().cast());
    let address = GetProcAddress(module, name.as_ptr().cast());
    if address.is_null() {
        None
    } else {
        Some(std::mem::transmute_copy(&address))
    }
}

struct App {
    runtime_size: Option<(f32, f32)>,
    destroy_window: Option<HWND>,
}
impl EventHandler for App {
    fn update(&mut self) {
        if let Some(hwnd) = self.destroy_window.take() {
            assert_ne!(unsafe { DestroyWindow(hwnd) }, 0);
            return;
        }
        if let Some(expected) = self.runtime_size.take() {
            assert_eq!(
                window::screen_size(),
                expected,
                "runtime request was scaled or truncated"
            );
            println!(
                "runtime physical request: {expected:?}, reported={:?}",
                window::screen_size()
            );
            window::order_quit();
        }
    }
    fn draw(&mut self) {
        panic!("draw ran after update ordered quit or destroyed the window");
    }
}

#[test]
fn startup_contract() {
    let mode = match std::env::var("MINIQUAD_DPI_STARTUP_CASE") {
        Ok(mode) => mode,
        Err(_) => {
            for case in [
                "standalone",
                "odd",
                "low",
                "host-pmv2",
                "host-pmv2-low",
                "host-system",
                "host-unaware",
                "host-thread-unaware",
                "fixed-size",
                "runtime-size",
                "destroy-window",
            ] {
                let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "startup_contract",
                        "--nocapture",
                        "--test-threads=1",
                    ])
                    .env("MINIQUAD_DPI_STARTUP_CASE", case)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap();
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
                while child.try_wait().unwrap().is_none() {
                    if std::time::Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        panic!("{}: startup did not finish within 20 seconds", case);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                let output = child.wait_with_output().unwrap();
                assert!(
                    output.status.success(),
                    "{case}:\n{}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                print!("{}", String::from_utf8_lossy(&output.stdout));
            }
            return;
        }
    };

    unsafe {
        // Dynamically resolve test probes too, so tests can still load on older
        // Windows. Modern awareness assertions require Windows 10 1703+.
        let get_dpi: unsafe extern "system" fn(HWND) -> UINT = match symbol(b"GetDpiForWindow\0") {
            Some(function) => function,
            None => {
                println!("SKIP {mode}: GetDpiForWindow unavailable");
                return;
            }
        };
        let set_context: unsafe extern "system" fn(DPI_AWARENESS_CONTEXT) -> BOOL =
            match symbol(b"SetProcessDpiAwarenessContext\0") {
                Some(function) => function,
                None => {
                    println!("SKIP {mode}: SetProcessDpiAwarenessContext unavailable");
                    return;
                }
            };
        let get_context: unsafe extern "system" fn(HWND) -> DPI_AWARENESS_CONTEXT =
            symbol(b"GetWindowDpiAwarenessContext\0").unwrap();
        let equal_context: unsafe extern "system" fn(
            DPI_AWARENESS_CONTEXT,
            DPI_AWARENESS_CONTEXT,
        ) -> BOOL = symbol(b"AreDpiAwarenessContextsEqual\0").unwrap();
        let get_thread_context: unsafe extern "system" fn() -> DPI_AWARENESS_CONTEXT =
            symbol(b"GetThreadDpiAwarenessContext\0").unwrap();

        let expected_context = match mode.as_str() {
            "host-pmv2" | "host-pmv2-low" | "manifest-pmv2" | "manifest-pmv2-low" => {
                Some(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
            }
            "host-system" => Some(DPI_AWARENESS_CONTEXT_SYSTEM_AWARE),
            "host-unaware" | "host-thread-unaware" => Some(DPI_AWARENESS_CONTEXT_UNAWARE),
            _ => None,
        };
        if mode == "host-thread-unaware" {
            assert_ne!(set_context(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2), 0);
            let set_thread_context: unsafe extern "system" fn(
                DPI_AWARENESS_CONTEXT,
            )
                -> DPI_AWARENESS_CONTEXT = symbol(b"SetThreadDpiAwarenessContext\0").unwrap();
            assert!(!set_thread_context(DPI_AWARENESS_CONTEXT_UNAWARE).is_null());
        } else if mode.starts_with("manifest-") {
            assert_ne!(
                equal_context(get_thread_context(), expected_context.unwrap()),
                0,
                "manifest must establish awareness before Miniquad starts"
            );
        } else if let Some(context) = expected_context {
            assert_ne!(
                set_context(context),
                0,
                "fresh child could not establish host awareness"
            );
        }

        let high_dpi = mode != "low" && !mode.ends_with("-low");
        let (width, height) = if mode == "odd" {
            (801, 601)
        } else {
            (800, 600)
        };
        let title: Vec<u16> = format!("Miniquad DPI startup test {mode}\0")
            .encode_utf16()
            .collect();
        miniquad::start(
            Conf {
                window_title: format!("Miniquad DPI startup test {mode}"),
                window_width: width,
                window_height: height,
                high_dpi,
                window_resizable: mode != "fixed-size",
                ..Default::default()
            },
            move || {
                let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
                assert!(!hwnd.is_null());
                let context = expected_context.unwrap_or(if high_dpi {
                    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
                } else {
                    DPI_AWARENESS_CONTEXT_UNAWARE
                });
                assert_ne!(
                    equal_context(get_context(hwnd), context),
                    0,
                    "{mode}: awareness changed unexpectedly"
                );
                let dpi = get_dpi(hwnd);
                assert!(dpi > 0);
                let scale = if high_dpi { dpi as f32 / 96.0 } else { 1.0 };
                assert_eq!(window::dpi_scale(), scale);
                assert_eq!(window::high_dpi(), high_dpi);
                let mut client: RECT = std::mem::zeroed();
                assert_ne!(GetClientRect(hwnd, &mut client), 0);
                let actual = (
                    (client.right - client.left) as f32,
                    (client.bottom - client.top) as f32,
                );
                // An independent OS query detects lost pixels in publication, not
                // just incorrect sizing arithmetic inside the startup helper.
                assert_eq!(
                    window::screen_size(),
                    actual,
                    "{mode}: first callback sees stale or truncated dimensions"
                );
                let requested = (
                    (width as f32 * scale).round(),
                    (height as f32 * scale).round(),
                );
                assert_eq!(
                    actual, requested,
                    "{mode}: unexpected client size (may be OS constrained)"
                );
                println!("{mode}: window_dpi={dpi} rendering_scale={scale} native_client={actual:?} reported={:?}", window::screen_size());
                let runtime_size = if mode == "runtime-size" {
                    window::set_window_size(1173, 853);
                    Some((1173.0, 853.0))
                } else if mode == "destroy-window" {
                    None
                } else {
                    window::order_quit();
                    None
                };
                Box::new(App {
                    runtime_size,
                    destroy_window: if mode == "destroy-window" {
                        Some(hwnd)
                    } else {
                        None
                    },
                })
            },
        );
    }
}
